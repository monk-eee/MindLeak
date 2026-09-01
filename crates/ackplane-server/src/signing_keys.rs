//! The registry that turns an envelope's `signing_key_id` into a key, and says
//! what that key's status was at the moment the envelope was accepted.

use std::time::SystemTime;

use thiserror::Error;
use tokio_postgres::{Client, Transaction};

/// One node's signing key, bound to the identity it may sign for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningKeyRecord {
    pub signing_key_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub public_key_fingerprint: String,
    pub activated_at: SystemTime,
    pub expires_at: Option<SystemTime>,
}

/// The binding an envelope claims, which a key must match to verify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeBinding<'a> {
    pub signing_key_id: &'a str,
    pub tenant_id: &'a str,
    pub repository_id: &'a str,
    pub producer_id: &'a str,
    /// When the ledger accepted the envelope, not when the lookup runs. A key
    /// revoked afterwards must not retroactively invalidate it.
    pub accepted_at: SystemTime,
}

/// Why a lookup did not yield a usable key, or that it did.
///
/// Separate variants rather than `Option`, because "no such key", "not this
/// node's key" and "revoked before this envelope" are different security
/// stories and collapsing them loses the one an auditor needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResolution {
    Resolved(SigningKeyRecord),
    Unknown,
    BindingMismatch,
    NotYetActive,
    Expired,
    Revoked,
    Retired,
}

#[derive(Debug, Error)]
pub enum SigningKeyError {
    #[error("signing key database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    /// Only reachable through a caller (e.g. `LedgerStore::resolve_signing_key`)
    /// that checks out its own connection before calling into this module --
    /// `register`/`retire`/`resolve` etc. take an already-open
    /// `&Transaction`/`&Client` and never see a pool themselves.
    #[error("signing key lookup could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
}

/// Record a key inside the caller's transaction, so a node becomes able to sign
/// exactly when its enrolment activates and never before.
pub async fn register(
    transaction: &Transaction<'_>,
    record: &SigningKeyRecord,
) -> Result<(), SigningKeyError> {
    transaction
        .execute(
            "INSERT INTO signing_keys (
                 signing_key_id, tenant_id, repository_id, node_id,
                 public_key, public_key_fingerprint, activated_at, expires_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (signing_key_id) DO NOTHING",
            &[
                &record.signing_key_id,
                &record.tenant_id,
                &record.repository_id,
                &record.node_id,
                &record.public_key,
                &record.public_key_fingerprint,
                &record.activated_at,
                &record.expires_at,
            ],
        )
        .await?;
    Ok(())
}

/// Fetch a key's record and lifecycle inside the caller's transaction, locking
/// the row so a concurrent rotation of the same key cannot race this one
/// (ADR-0085 decision 7). `None` means no such key is registered at all.
pub async fn fetch_lifecycle_for_update(
    transaction: &Transaction<'_>,
    signing_key_id: &str,
) -> Result<Option<SigningKeyLifecycle>, SigningKeyError> {
    let row = transaction
        .query_opt(
            "SELECT signing_key_id, tenant_id, repository_id, node_id, public_key, \
             public_key_fingerprint, activated_at, expires_at, retired_at, revoked_at \
             FROM signing_keys WHERE signing_key_id = $1 FOR UPDATE",
            &[&signing_key_id],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(SigningKeyLifecycle {
        record: SigningKeyRecord {
            signing_key_id: row.get(0),
            tenant_id: row.get(1),
            repository_id: row.get(2),
            node_id: row.get(3),
            public_key: row.get(4),
            public_key_fingerprint: row.get(5),
            activated_at: row.get(6),
            expires_at: row.get(7),
        },
        retired_at: row.get(8),
        revoked_at: row.get(9),
    }))
}

/// Every key ever registered for a repository, newest first, for a read-only
/// health view (the Bridge repository detail resource) -- not `FOR UPDATE`,
/// since nothing here is about to mutate a row.
pub async fn for_repository(
    client: &Client,
    tenant_id: &str,
    repository_id: &str,
) -> Result<Vec<SigningKeyLifecycle>, SigningKeyError> {
    let rows = client
        .query(
            "SELECT signing_key_id, tenant_id, repository_id, node_id, public_key, \
             public_key_fingerprint, activated_at, expires_at, retired_at, revoked_at \
             FROM signing_keys WHERE tenant_id = $1 AND repository_id = $2 \
             ORDER BY activated_at DESC",
            &[&tenant_id, &repository_id],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| SigningKeyLifecycle {
            record: SigningKeyRecord {
                signing_key_id: row.get(0),
                tenant_id: row.get(1),
                repository_id: row.get(2),
                node_id: row.get(3),
                public_key: row.get(4),
                public_key_fingerprint: row.get(5),
                activated_at: row.get(6),
                expires_at: row.get(7),
            },
            retired_at: row.get(8),
            revoked_at: row.get(9),
        })
        .collect())
}

/// Whether a signing key id is already registered, for rejecting a rotation
/// whose proposed successor id collides with an existing binding.
pub async fn key_exists(
    transaction: &Transaction<'_>,
    signing_key_id: &str,
) -> Result<bool, SigningKeyError> {
    Ok(transaction
        .query_opt(
            "SELECT 1 FROM signing_keys WHERE signing_key_id = $1",
            &[&signing_key_id],
        )
        .await?
        .is_some())
}

/// Mark a key retired at `retired_at`, ending its authority for records
/// accepted afterward while everything it already signed stays resolvable
/// (ADR-0085 decision 7's bounded rotation overlap).
pub async fn retire(
    transaction: &Transaction<'_>,
    signing_key_id: &str,
    retired_at: SystemTime,
) -> Result<(), SigningKeyError> {
    transaction
        .execute(
            "UPDATE signing_keys SET retired_at = $2 WHERE signing_key_id = $1",
            &[&signing_key_id, &retired_at],
        )
        .await?;
    Ok(())
}

/// An authorised administrator or incident workflow's revocation of a node's
/// signing key (ADR-0085 decision 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRevocation {
    pub signing_key_id: String,
    pub reason: String,
}

/// Revoke a key, immediately for new authority. First revocation wins: once
/// `revoked_at` is set it is never moved, because a later call moving it
/// later would resurrect authority for the gap between the two timestamps.
/// Returns `true` iff this call performed the revocation, `false` if the key
/// was already revoked.
pub async fn revoke(
    transaction: &Transaction<'_>,
    revocation: &KeyRevocation,
    revoked_at: SystemTime,
) -> Result<bool, SigningKeyError> {
    let updated = transaction
        .execute(
            "UPDATE signing_keys SET revoked_at = $2, revocation_reason = $3 \
             WHERE signing_key_id = $1 AND revoked_at IS NULL",
            &[&revocation.signing_key_id, &revoked_at, &revocation.reason],
        )
        .await?;
    Ok(updated > 0)
}

/// A key together with the lifecycle events that may have ended its authority.
///
/// Separate from `SigningKeyRecord` because these are the fields a verifier
/// must never treat as part of the key: they answer "was it valid then", not
/// "what is it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningKeyLifecycle {
    pub record: SigningKeyRecord,
    pub retired_at: Option<SystemTime>,
    pub revoked_at: Option<SystemTime>,
}

/// Decide what a claimed key was worth at the moment an envelope was accepted.
///
/// Pure, so every lifecycle rule below is provable with no PostgreSQL, no
/// container and no network (ADR-0088 clause 2) — the same split `sync::translate`
/// uses. The fetch is the easy half; this is the half that decides whether
/// evidence is trustworthy.
pub fn judge(lifecycle: &SigningKeyLifecycle, binding: &EnvelopeBinding<'_>) -> KeyResolution {
    let record = &lifecycle.record;

    // Before any lifecycle question: a key belonging to another node is not an
    // expired key, it is one this envelope was never entitled to use.
    if record.tenant_id != binding.tenant_id
        || record.repository_id != binding.repository_id
        || record.node_id != binding.producer_id
    {
        return KeyResolution::BindingMismatch;
    }

    // Every window is judged against acceptance, never against now. A key
    // revoked an hour after an envelope arrived was valid when it signed it,
    // and ADR-0084 requires that envelope to stay resolvable afterwards.
    if binding.accepted_at < record.activated_at {
        return KeyResolution::NotYetActive;
    }
    if lifecycle
        .revoked_at
        .is_some_and(|revoked| binding.accepted_at >= revoked)
    {
        return KeyResolution::Revoked;
    }
    if record
        .expires_at
        .is_some_and(|expires| binding.accepted_at >= expires)
    {
        return KeyResolution::Expired;
    }
    if lifecycle
        .retired_at
        .is_some_and(|retired| binding.accepted_at >= retired)
    {
        return KeyResolution::Retired;
    }
    KeyResolution::Resolved(record.clone())
}

/// Resolve the key an envelope claims, judged as of when it was accepted.
pub async fn resolve(
    client: &Client,
    binding: &EnvelopeBinding<'_>,
) -> Result<KeyResolution, SigningKeyError> {
    let row = client
        .query_opt(
            "SELECT signing_key_id, tenant_id, repository_id, node_id, public_key, \
             public_key_fingerprint, activated_at, expires_at, retired_at, revoked_at \
             FROM signing_keys WHERE signing_key_id = $1",
            &[&binding.signing_key_id],
        )
        .await?;
    let Some(row) = row else {
        return Ok(KeyResolution::Unknown);
    };

    Ok(judge(
        &SigningKeyLifecycle {
            record: SigningKeyRecord {
                signing_key_id: row.get(0),
                tenant_id: row.get(1),
                repository_id: row.get(2),
                node_id: row.get(3),
                public_key: row.get(4),
                public_key_fingerprint: row.get(5),
                activated_at: row.get(6),
                expires_at: row.get(7),
            },
            retired_at: row.get(8),
            revoked_at: row.get(9),
        },
        binding,
    ))
}

/// Find the lifecycle of the signing key registered for a specific candidate
/// binding, keyed on the fingerprint the candidate itself computed and
/// presented -- not a `signing_key_id`, because `CheckEnrollmentStatus`
/// (ADR-0122) exists for exactly the callers who have none to offer: a
/// pre-activation candidate has never been assigned one, and a caller
/// presenting a fingerprint from before its most recent rotation was assigned
/// a *different* id than its current key holds.
///
/// This is the authoritative source for a binding's current status once it
/// has ever been activated: unlike `enrollment_requests.state`, retirement and
/// revocation are written directly to this table's `retired_at`/`revoked_at`
/// columns as they happen, so a caller must consult this before falling back
/// to the enrollment request's own (potentially stale) state column.
///
/// Not `FOR UPDATE`: a status check observes, it never contends with a
/// concurrent rotation or revocation for the row. `LIMIT 1` guards the
/// unlikely case of the same key material being registered under two
/// different `signing_key_id`s for the same node (e.g. revoke, then
/// re-enroll with identical key bytes); the most recently activated binding
/// is the one a fresh status check should answer for.
pub async fn lifecycle_for_binding(
    client: &Client,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    public_key_fingerprint: &str,
) -> Result<Option<SigningKeyLifecycle>, SigningKeyError> {
    let row = client
        .query_opt(
            "SELECT signing_key_id, tenant_id, repository_id, node_id, public_key, \
             public_key_fingerprint, activated_at, expires_at, retired_at, revoked_at \
             FROM signing_keys WHERE tenant_id = $1 AND repository_id = $2 \
             AND node_id = $3 AND public_key_fingerprint = $4 \
             ORDER BY activated_at DESC LIMIT 1",
            &[
                &tenant_id,
                &repository_id,
                &node_id,
                &public_key_fingerprint,
            ],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(SigningKeyLifecycle {
        record: SigningKeyRecord {
            signing_key_id: row.get(0),
            tenant_id: row.get(1),
            repository_id: row.get(2),
            node_id: row.get(3),
            public_key: row.get(4),
            public_key_fingerprint: row.get(5),
            activated_at: row.get(6),
            expires_at: row.get(7),
        },
        retired_at: row.get(8),
        revoked_at: row.get(9),
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const ACTIVATED: Duration = Duration::from_secs(1_000);

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn lifecycle() -> SigningKeyLifecycle {
        SigningKeyLifecycle {
            record: SigningKeyRecord {
                signing_key_id: "signing-key-a".to_owned(),
                tenant_id: "tenant-a".to_owned(),
                repository_id: "repository-a".to_owned(),
                node_id: "node-a".to_owned(),
                public_key: vec![7; 32],
                public_key_fingerprint: "ed25519:7f3a".to_owned(),
                activated_at: SystemTime::UNIX_EPOCH + ACTIVATED,
                expires_at: None,
            },
            retired_at: None,
            revoked_at: None,
        }
    }

    fn binding(accepted_at: SystemTime) -> EnvelopeBinding<'static> {
        EnvelopeBinding {
            signing_key_id: "signing-key-a",
            tenant_id: "tenant-a",
            repository_id: "repository-a",
            producer_id: "node-a",
            accepted_at,
        }
    }

    #[test]
    fn a_live_key_resolves_for_the_node_it_was_issued_to() {
        assert_eq!(
            judge(&lifecycle(), &binding(at(2_000))),
            KeyResolution::Resolved(lifecycle().record)
        );
    }

    /// The load-bearing rule of the whole registry. ADR-0084 requires a
    /// previously accepted envelope to stay resolvable after the key is later
    /// revoked, so revocation must be judged against acceptance rather than now.
    /// Judged against now, every historical receipt would silently become
    /// unverifiable the moment a node was decommissioned.
    #[test]
    fn a_key_revoked_later_still_resolves_for_an_envelope_accepted_before_it() {
        let mut lifecycle = lifecycle();
        lifecycle.revoked_at = Some(at(5_000));

        assert_eq!(
            judge(&lifecycle, &binding(at(2_000))),
            KeyResolution::Resolved(lifecycle.record.clone())
        );
        assert_eq!(
            judge(&lifecycle, &binding(at(5_000))),
            KeyResolution::Revoked
        );
        assert_eq!(
            judge(&lifecycle, &binding(at(9_000))),
            KeyResolution::Revoked
        );
    }

    /// Rotation overlap (ADR-0085 decision 7): the retiring key must keep
    /// settling in-flight records, so retirement ends its authority at a point
    /// in time instead of deleting it.
    #[test]
    fn a_retired_key_still_settles_records_signed_before_it_retired() {
        let mut lifecycle = lifecycle();
        lifecycle.retired_at = Some(at(4_000));

        assert_eq!(
            judge(&lifecycle, &binding(at(3_999))),
            KeyResolution::Resolved(lifecycle.record.clone())
        );
        assert_eq!(
            judge(&lifecycle, &binding(at(4_000))),
            KeyResolution::Retired
        );
    }

    #[test]
    fn an_expiry_ends_authority_at_the_moment_it_falls_due() {
        let mut lifecycle = lifecycle();
        lifecycle.record.expires_at = Some(at(3_000));

        assert_eq!(
            judge(&lifecycle, &binding(at(2_999))),
            KeyResolution::Resolved(lifecycle.record.clone())
        );
        assert_eq!(
            judge(&lifecycle, &binding(at(3_000))),
            KeyResolution::Expired
        );
    }

    #[test]
    fn a_key_cannot_sign_for_an_envelope_older_than_its_activation() {
        assert_eq!(
            judge(&lifecycle(), &binding(at(999))),
            KeyResolution::NotYetActive
        );
    }

    /// Each field is checked on its own: a single combined comparison would
    /// pass whenever any one of them happened to match.
    #[test]
    fn a_key_never_signs_for_an_identity_it_was_not_issued_to() {
        for wrong in [
            EnvelopeBinding {
                tenant_id: "tenant-b",
                ..binding(at(2_000))
            },
            EnvelopeBinding {
                repository_id: "repository-b",
                ..binding(at(2_000))
            },
            EnvelopeBinding {
                producer_id: "node-b",
                ..binding(at(2_000))
            },
        ] {
            assert_eq!(judge(&lifecycle(), &wrong), KeyResolution::BindingMismatch);
        }
    }

    /// A revoked key belonging to another node must report the mismatch, not
    /// the revocation: answering "revoked" would confirm the key exists and
    /// describe a lifecycle the caller has no standing to ask about.
    #[test]
    fn a_foreign_key_reports_the_mismatch_rather_than_its_lifecycle() {
        let mut lifecycle = lifecycle();
        lifecycle.revoked_at = Some(at(1_500));

        let wrong = EnvelopeBinding {
            producer_id: "node-b",
            ..binding(at(2_000))
        };

        assert_eq!(judge(&lifecycle, &wrong), KeyResolution::BindingMismatch);
    }

    /// Applies just this module's migration, standalone: `revoke`/`register`
    /// need a real `signing_keys` table but nothing here depends on the
    /// enrollment ceremony's tables.
    async fn connect_and_migrate(database_url: &str) -> Client {
        let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
            .await
            .expect("test database connects");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .batch_execute(include_str!("../migrations/0004_signing_keys.sql"))
            .await
            .expect("signing_keys migration applies");
        client
    }

    fn test_record(signing_key_id: &str) -> SigningKeyRecord {
        SigningKeyRecord {
            signing_key_id: signing_key_id.to_owned(),
            tenant_id: "tenant-revoke-test".to_owned(),
            repository_id: "repository-revoke-test".to_owned(),
            node_id: "node-revoke-test".to_owned(),
            public_key: vec![9; 32],
            // Derived from the caller's already-unique `signing_key_id`
            // rather than a fixed literal: the unique constraint this row
            // must satisfy is (tenant_id, repository_id, node_id,
            // public_key_fingerprint, activated_at), which does not include
            // signing_key_id, so two tests sharing a fixed fingerprint
            // collide even though their key ids differ.
            public_key_fingerprint: format!("ed25519:{signing_key_id}"),
            activated_at: at(1_000),
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn revoking_a_key_ends_its_authority_immediately() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut client = connect_and_migrate(&database_url).await;
        let record = test_record(&format!(
            "signing-key-revoke-{}",
            crate::test_support::uuid_ish()
        ));

        let transaction = client.transaction().await.unwrap();
        register(&transaction, &record).await.unwrap();
        transaction.commit().await.unwrap();

        let revoked_at = at(5_000);
        let transaction = client.transaction().await.unwrap();
        let performed = revoke(
            &transaction,
            &KeyRevocation {
                signing_key_id: record.signing_key_id.clone(),
                reason: "suspected compromise".to_owned(),
            },
            revoked_at,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(performed, "the first revocation must take effect");

        let resolution = resolve(
            &client,
            &EnvelopeBinding {
                signing_key_id: &record.signing_key_id,
                tenant_id: &record.tenant_id,
                repository_id: &record.repository_id,
                producer_id: &record.node_id,
                accepted_at: at(5_500),
            },
        )
        .await
        .unwrap();
        assert_eq!(resolution, KeyResolution::Revoked);
    }

    /// A second revocation must not move `revoked_at` later, or the gap
    /// between the two timestamps would resurrect authority that was already
    /// meant to have ended.
    #[tokio::test]
    async fn a_second_revocation_is_a_no_op_and_never_moves_revoked_at() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut client = connect_and_migrate(&database_url).await;
        let record = test_record(&format!(
            "signing-key-double-revoke-{}",
            crate::test_support::uuid_ish()
        ));

        let transaction = client.transaction().await.unwrap();
        register(&transaction, &record).await.unwrap();
        transaction.commit().await.unwrap();

        let first_revocation = at(5_000);
        let transaction = client.transaction().await.unwrap();
        revoke(
            &transaction,
            &KeyRevocation {
                signing_key_id: record.signing_key_id.clone(),
                reason: "first reason".to_owned(),
            },
            first_revocation,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let later = at(9_000);
        let transaction = client.transaction().await.unwrap();
        let performed = revoke(
            &transaction,
            &KeyRevocation {
                signing_key_id: record.signing_key_id.clone(),
                reason: "a later, different reason".to_owned(),
            },
            later,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(!performed, "a second revocation must be a no-op");

        // Judged as of an instant BETWEEN the two attempted timestamps: if the
        // second call had moved revoked_at to `later`, this key would wrongly
        // resolve as still authoritative here.
        let resolution = resolve(
            &client,
            &EnvelopeBinding {
                signing_key_id: &record.signing_key_id,
                tenant_id: &record.tenant_id,
                repository_id: &record.repository_id,
                producer_id: &record.node_id,
                accepted_at: at(6_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(resolution, KeyResolution::Revoked);
    }

    /// A separate tenant/repository per test, not `test_record`'s shared
    /// fixed pair -- `for_repository` filters by them, and sharing would let
    /// this test see a sibling's rows in the same persistent database.
    fn health_view_record(
        unique: &str,
        signing_key_id: &str,
        activated_at: SystemTime,
    ) -> SigningKeyRecord {
        SigningKeyRecord {
            signing_key_id: signing_key_id.to_owned(),
            tenant_id: format!("tenant-health-{unique}"),
            repository_id: format!("repository-health-{unique}"),
            node_id: "node-health".to_owned(),
            public_key: vec![3; 32],
            public_key_fingerprint: format!("ed25519:{signing_key_id}"),
            activated_at,
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn for_repository_returns_every_key_newest_first_and_none_from_another_repository() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut client = connect_and_migrate(&database_url).await;
        let unique = crate::test_support::uuid_ish().to_string();
        let older = health_view_record(
            &unique,
            &format!("signing-key-health-older-{unique}"),
            at(1_000),
        );
        let newer = health_view_record(
            &unique,
            &format!("signing-key-health-newer-{unique}"),
            at(2_000),
        );
        let other_repository = SigningKeyRecord {
            repository_id: format!("repository-health-other-{unique}"),
            ..health_view_record(
                &unique,
                &format!("signing-key-health-other-{unique}"),
                at(1_500),
            )
        };

        let transaction = client.transaction().await.unwrap();
        register(&transaction, &older).await.unwrap();
        register(&transaction, &newer).await.unwrap();
        register(&transaction, &other_repository).await.unwrap();
        transaction.commit().await.unwrap();

        let keys = for_repository(&client, &older.tenant_id, &older.repository_id)
            .await
            .unwrap();
        assert_eq!(
            keys.iter()
                .map(|k| &k.record.signing_key_id)
                .collect::<Vec<_>>(),
            vec![&newer.signing_key_id, &older.signing_key_id],
            "newest activated_at first, and never a key registered under a different repository_id"
        );
    }
}
