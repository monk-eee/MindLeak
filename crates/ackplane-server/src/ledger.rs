//! The ledger schema and its idempotent append transaction (ADR-0086 clauses
//! 4, 5, 6, 11).
//!
//! Every table this module reads or writes is defined in
//! `migrations/0001_ledger.sql`, applied idempotently by [`LedgerStore::connect`]
//! (every statement is `CREATE TABLE IF NOT EXISTS`, so re-running it is safe).
//! There is only one migration so far; ADR-0086 clause 13's expand/backfill/
//! verify/contract sequence is for schema evolution this crate does not have
//! yet.
//!
//! # Testing without a database (ADR-0088 clause 2)
//!
//! The repository-local planes — including `cargo test --workspace` — must run
//! on a machine with no Docker, no PostgreSQL, and no network, and CI proves
//! that with a job that has none available. The tests below that exercise a
//! real database are therefore opt-in: they read `ACKPLANE_TEST_DATABASE_URL`
//! and skip (print a notice, return early) when it is absent, rather than
//! attempting a connection that job could never make. Nothing in this module
//! connects to anything unless a caller supplies a URL — [`LedgerStore::connect`]
//! takes one explicitly and is never invoked at module load. A dedicated CI job
//! that provisions PostgreSQL and sets that variable is a natural follow-up,
//! deliberately left to its own change rather than bundled with this one.

use tokio_postgres::{Client, NoTls};

use thiserror::Error;

const MIGRATION: &str = include_str!("../migrations/0001_ledger.sql");

/// Which class of provenance produced an envelope, mirroring ADR-0083's
/// `ProvenanceClass`. Stored as a small integer so the column stays an
/// efficient, indexable discriminant rather than a repeated text label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceClass {
    UnverifiedAttribution,
    EnrolledNode,
    AuthenticatedPrincipal,
    ProviderAttested,
}

impl ProvenanceClass {
    fn as_i16(self) -> i16 {
        match self {
            Self::UnverifiedAttribution => 1,
            Self::EnrolledNode => 2,
            Self::AuthenticatedPrincipal => 3,
            Self::ProviderAttested => 4,
        }
    }

    /// No production path reads a stored provenance class back yet — that
    /// belongs to whichever task first needs to (e.g. a projection reader).
    /// Kept for the round-trip test, which is what actually needs it.
    #[cfg(test)]
    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::UnverifiedAttribution),
            2 => Some(Self::EnrolledNode),
            3 => Some(Self::AuthenticatedPrincipal),
            4 => Some(Self::ProviderAttested),
            _ => None,
        }
    }
}

/// The durable dedup key from ADR-0083 clause 7: `(tenant_id, repository_id,
/// producer_id, producer_sequence)`. Repeating it with the same envelope
/// digest is a retry; repeating it with a different digest is a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub tenant_id: String,
    pub repository_id: String,
    pub producer_id: String,
    pub producer_sequence: i64,
}

/// A record a repository node is asking Ackplane to append. Deliberately a
/// plain storage-layer type rather than the generated Protobuf message:
/// mapping the wire contract onto this is the gRPC handler's job (ADR-0083),
/// not the ledger's, so the two can change independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope {
    pub key: DedupKey,
    pub payload: Vec<u8>,
    pub payload_digest: Vec<u8>,
    pub schema_version: String,
    pub occurred_at: std::time::SystemTime,
    pub payload_type: String,
    pub previous_envelope_digest: Option<Vec<u8>>,
    pub signing_key_id: Option<String>,
    pub signature: Option<Vec<u8>>,
    pub provenance: ProvenanceClass,
}

/// Whether an accepted record is new or the reply to an identical retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Accepted { position: i64 },
    Duplicate { position: i64 },
}

#[derive(Debug, Error)]
pub enum AppendError {
    /// The same dedup key was submitted with a different envelope digest.
    /// Non-retryable: the caller sent two different records under one
    /// identity, and no amount of retrying resolves that (ADR-0083 clause 7).
    #[error(
        "producer_sequence {sequence} for producer {producer_id} was already accepted with a \
         different envelope digest; this is a non-retryable conflict"
    )]
    Conflict { producer_id: String, sequence: i64 },
    #[error("ledger database error: {0}")]
    Database(#[from] tokio_postgres::Error),
}

/// A connection to Ackplane's authoritative store (ADR-0086 clause 1).
pub struct LedgerStore {
    client: Client,
}

impl LedgerStore {
    /// Connect and apply the schema. Every statement in the migration is
    /// idempotent, so this is safe to call on every process start rather than
    /// only on a fresh database (ADR-0088 clause 5 still reserves the actual
    /// *first* application of a migration to a one-shot `migrate` step in the
    /// Compose topology; this repeats safely, it does not race a concurrent
    /// first application).
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane ledger connection closed with an error");
            }
        });
        client.batch_execute(MIGRATION).await?;
        Ok(Self { client })
    }

    /// Resolve the signing key an envelope claims, judged as of acceptance.
    ///
    /// On the store because the store owns the connection; the decision itself
    /// lives in `signing_keys` and is pure.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, crate::signing_keys::SigningKeyError> {
        crate::signing_keys::resolve(&self.client, binding).await
    }

    /// The transaction ADR-0086 clauses 4, 5, 6 and 11 describe: lock the
    /// stream head, check the producer sequence, verify the digest, append at
    /// most one record, create its receipt, and advance the head — all before
    /// anything is returned, so a caller never observes a position that was
    /// not durably committed.
    pub async fn append(&mut self, envelope: &EventEnvelope) -> Result<AppendOutcome, AppendError> {
        let key = &envelope.key;
        let transaction = self.client.transaction().await?;

        // A same-key retry is answered from the row already written, without
        // touching the stream head at all: it neither allocates a position
        // nor needs one locked (ADR-0083 clause 7).
        let existing = transaction
            .query_opt(
                "SELECT stream_position, payload_digest FROM ledger_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND producer_id = $3 \
                   AND producer_sequence = $4",
                &[
                    &key.tenant_id,
                    &key.repository_id,
                    &key.producer_id,
                    &key.producer_sequence,
                ],
            )
            .await?;
        if let Some(row) = existing {
            let stored_position: i64 = row.get(0);
            let stored_digest: Vec<u8> = row.get(1);
            if stored_digest == envelope.payload_digest {
                return Ok(AppendOutcome::Duplicate {
                    position: stored_position,
                });
            }
            return Err(AppendError::Conflict {
                producer_id: key.producer_id.clone(),
                sequence: key.producer_sequence,
            });
        }

        // Get-or-create the stream head and lock its row in one statement: an
        // `UPDATE` (even one that leaves the value unchanged) takes the same
        // row lock a `SELECT ... FOR UPDATE` would, so a concurrent append on
        // the same stream blocks here until this transaction commits or rolls
        // back. Unrelated streams are untouched (ADR-0086 clause 4).
        let head_row = transaction
            .query_one(
                "INSERT INTO stream_heads (tenant_id, repository_id, position) \
                 VALUES ($1, $2, 0) \
                 ON CONFLICT (tenant_id, repository_id) \
                 DO UPDATE SET position = stream_heads.position \
                 RETURNING position",
                &[&key.tenant_id, &key.repository_id],
            )
            .await?;
        let locked_position: i64 = head_row.get(0);
        let next_position = locked_position + 1;

        let occurred_at: std::time::SystemTime = envelope.occurred_at;
        transaction
            .execute(
                "INSERT INTO ledger_records (
                     tenant_id, repository_id, producer_id, producer_sequence,
                     stream_position, payload, payload_digest, schema_version,
                     occurred_at, payload_type, previous_envelope_digest,
                     signing_key_id, signature, provenance_class
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
                &[
                    &key.tenant_id,
                    &key.repository_id,
                    &key.producer_id,
                    &key.producer_sequence,
                    &next_position,
                    &envelope.payload,
                    &envelope.payload_digest,
                    &envelope.schema_version,
                    &occurred_at,
                    &envelope.payload_type,
                    &envelope.previous_envelope_digest,
                    &envelope.signing_key_id,
                    &envelope.signature,
                    &envelope.provenance.as_i16(),
                ],
            )
            .await?;

        transaction
            .execute(
                "INSERT INTO ledger_receipts (
                     tenant_id, repository_id, producer_id, producer_sequence,
                     stream_position, disposition, payload_digest
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7)",
                &[
                    &key.tenant_id,
                    &key.repository_id,
                    &key.producer_id,
                    &key.producer_sequence,
                    &next_position,
                    &1i16, // Accepted
                    &envelope.payload_digest,
                ],
            )
            .await?;

        transaction
            .execute(
                "UPDATE stream_heads SET position = $3 \
                 WHERE tenant_id = $1 AND repository_id = $2",
                &[&key.tenant_id, &key.repository_id, &next_position],
            )
            .await?;

        transaction.commit().await?;
        Ok(AppendOutcome::Accepted {
            position: next_position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(key: DedupKey, digest: &[u8]) -> EventEnvelope {
        EventEnvelope {
            key,
            payload: b"payload".to_vec(),
            payload_digest: digest.to_vec(),
            schema_version: "v1".to_string(),
            occurred_at: std::time::SystemTime::now(),
            payload_type: "mindleak.ackplane.v1.EventEnvelope".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        }
    }

    #[test]
    fn provenance_class_round_trips_through_its_stored_integer() {
        for class in [
            ProvenanceClass::UnverifiedAttribution,
            ProvenanceClass::EnrolledNode,
            ProvenanceClass::AuthenticatedPrincipal,
            ProvenanceClass::ProviderAttested,
        ] {
            assert_eq!(ProvenanceClass::from_i16(class.as_i16()), Some(class));
        }
    }

    #[test]
    fn an_unrecognised_stored_value_is_not_silently_mapped_to_a_class() {
        assert_eq!(ProvenanceClass::from_i16(0), None);
        assert_eq!(ProvenanceClass::from_i16(5), None);
    }

    /// Real-database coverage. Opt-in via `ACKPLANE_TEST_DATABASE_URL`, and
    /// skipped (not failed) when it is unset — see the module doc for why:
    /// `cargo test --workspace` must still pass on a machine with no
    /// PostgreSQL (ADR-0088 clause 2).
    macro_rules! require_test_database {
        () => {
            match std::env::var("ACKPLANE_TEST_DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!(
                        "skipping: ACKPLANE_TEST_DATABASE_URL is not set (ADR-0088 clause 2 keeps \
                         this opt-in rather than requiring PostgreSQL in default CI)"
                    );
                    return;
                }
            }
        };
    }

    #[tokio::test]
    async fn a_fresh_stream_assigns_increasing_positions() {
        let url = require_test_database!();
        let mut store = LedgerStore::connect(&url).await.expect("connect");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();
        let producer = "producer-a".to_string();

        let first = envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: producer.clone(),
                producer_sequence: 1,
            },
            b"digest-1",
        );
        let second = envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: producer.clone(),
                producer_sequence: 2,
            },
            b"digest-2",
        );

        assert_eq!(
            store.append(&first).await.unwrap(),
            AppendOutcome::Accepted { position: 1 }
        );
        assert_eq!(
            store.append(&second).await.unwrap(),
            AppendOutcome::Accepted { position: 2 }
        );
    }

    #[tokio::test]
    async fn a_same_key_same_digest_retry_returns_the_original_position_and_appends_nothing() {
        let url = require_test_database!();
        let mut store = LedgerStore::connect(&url).await.expect("connect");
        let key = DedupKey {
            tenant_id: format!("t-{}", uuid_ish()),
            repository_id: "repo-a".to_string(),
            producer_id: "producer-a".to_string(),
            producer_sequence: 1,
        };
        let record = envelope(key.clone(), b"digest-1");

        let first = store.append(&record).await.unwrap();
        assert_eq!(first, AppendOutcome::Accepted { position: 1 });

        let retry = store.append(&record).await.unwrap();
        assert_eq!(retry, AppendOutcome::Duplicate { position: 1 });
    }

    #[tokio::test]
    async fn a_same_key_different_digest_retry_is_a_non_retryable_conflict() {
        let url = require_test_database!();
        let mut store = LedgerStore::connect(&url).await.expect("connect");
        let key = DedupKey {
            tenant_id: format!("t-{}", uuid_ish()),
            repository_id: "repo-a".to_string(),
            producer_id: "producer-a".to_string(),
            producer_sequence: 1,
        };

        store
            .append(&envelope(key.clone(), b"digest-1"))
            .await
            .unwrap();

        let conflict = store
            .append(&envelope(key.clone(), b"digest-2"))
            .await
            .unwrap_err();
        assert!(matches!(conflict, AppendError::Conflict { .. }));
    }

    #[tokio::test]
    async fn two_repositories_advance_independent_stream_positions() {
        let url = require_test_database!();
        let mut store = LedgerStore::connect(&url).await.expect("connect");
        let tenant = format!("t-{}", uuid_ish());

        let repo_a = envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: "repo-a".to_string(),
                producer_id: "producer-a".to_string(),
                producer_sequence: 1,
            },
            b"digest-a",
        );
        let repo_b = envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: "repo-b".to_string(),
                producer_id: "producer-b".to_string(),
                producer_sequence: 1,
            },
            b"digest-b",
        );

        assert_eq!(
            store.append(&repo_a).await.unwrap(),
            AppendOutcome::Accepted { position: 1 }
        );
        assert_eq!(
            store.append(&repo_b).await.unwrap(),
            AppendOutcome::Accepted { position: 1 }
        );
    }

    use crate::test_support::uuid_ish;
}
