//! Project the Ackplane graph from ledger records (ADR-0087 clauses 1, 2, 3,
//! 6, 10).
//!
//! Every table here is defined in `migrations/0002_projection.sql`, applied
//! idempotently by [`Projector::connect`]. [`Projector::rebuild`] is the only
//! writer: it drops a repository's projected nodes and edges and replays its
//! committed [`STRUCTURAL_FACT_PAYLOAD_TYPE`] ledger records in stream order,
//! so a rebuild always reproduces the same projection from the same ledger
//! (clause 1) — proven by [`a_rebuild_reproduces_the_same_projection_from_the_same_ledger`].
//!
//! [`Projector::bounded_neighborhood`] is the read side: an ordinary
//! recursive-CTE traversal (clause 2, no extension required) that honours the
//! same bounded, best-first contract `mindleak-core`'s `GraphStore` already
//! implements — seed set, max depth, max admitted nodes, per-node fanout
//! limited to the strongest edges, dangling edges dropped (clause 3) — with
//! effective weight computed in the query, never stored (clause 6). It also
//! returns the projection's freshness (clause 10): the ledger stream position
//! folded in and when that rebuild ran, so a stale or unbuilt projection is
//! legible rather than presented as current fact.
//!
//! # Testing without a database (ADR-0088 clause 2)
//!
//! As in [`crate::ledger`], the tests that exercise a real database are
//! opt-in via `ACKPLANE_TEST_DATABASE_URL` and skip (rather than fail) when it
//! is absent, so `cargo test --workspace` keeps passing with no PostgreSQL,
//! Docker, or network reachable.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../../migrations/0002_projection.sql");

/// The `payload_type` a ledger record must carry to be folded into the graph
/// projection. Any other payload type is ignored by [`Projector::rebuild`]
/// rather than rejected — the ledger accepts every accepted record type, and
/// deciding which ones are graph-relevant is the projector's job, not the
/// ledger's.
pub const STRUCTURAL_FACT_PAYLOAD_TYPE: &str = "structural_fact";

/// One edge a [`StructuralFact`] declares from its node.
///
/// Deliberately a plain, JSON-encoded storage-layer type, not the generated
/// Protobuf message: the wire contract for what a repository node actually
/// publishes belongs to ADR-0083's gRPC handler task, which can define and
/// change it independently of how the projector folds an accepted payload
/// into these tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralEdgeFact {
    pub target_id: String,
    pub relation: String,
    pub base_weight: f64,
    pub half_life_hours: f64,
}

/// A minimised structural summary (ADR-0087 clause 7): one node and the
/// edges it declares, published under [`STRUCTURAL_FACT_PAYLOAD_TYPE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralFact {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    #[serde(default)]
    pub edges: Vec<StructuralEdgeFact>,
}

/// Row counts left in a repository's projection after [`Projector::rebuild`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionSummary {
    pub nodes: i64,
    pub edges: i64,
    pub stream_position: i64,
}

/// A projected node returned by [`Projector::bounded_neighborhood`], with the
/// traversal depth it was admitted at.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedNode {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    pub depth: i32,
}

/// A projected edge returned by [`Projector::bounded_neighborhood`]. Effective
/// weight is intentionally absent: it is a function of `now`, so a caller
/// recomputes it from `base_weight`/`half_life_hours`/`updated_at` rather
/// than treating a returned number as durable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub base_weight: f64,
    pub half_life_hours: f64,
    pub updated_at: SystemTime,
}

/// A repository's projection freshness (ADR-0087 clause 10): `None` means the
/// repository has never been projected, not position zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionFreshness {
    pub stream_position: i64,
    pub projected_at: SystemTime,
}

/// A bounded neighbourhood around a seed set, carrying the freshness of the
/// projection it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedNeighborhood {
    pub nodes: Vec<ProjectedNode>,
    pub edges: Vec<ProjectedEdge>,
    pub freshness: Option<ProjectionFreshness>,
}

/// One repository whose committed structural facts are ahead of its
/// projection checkpoint (ADR-0086 clause 9): either it has never been
/// projected, or the ledger has moved past the position it was last
/// projected at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleProjection {
    pub tenant_id: String,
    pub repository_id: String,
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("structural fact at stream position {position} could not be decoded: {source}")]
    MalformedFact {
        position: i64,
        #[source]
        source: serde_json::Error,
    },
    #[error("projection database error: {0}")]
    Database(#[from] tokio_postgres::Error),
}

/// Whether a SQLSTATE is PostgreSQL's own deadlock detection (40P01) — the
/// one server-reported error its own documentation recommends retrying
/// rather than treating as permanent.
fn is_deadlock(code: &tokio_postgres::error::SqlState) -> bool {
    *code == tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED
}

/// A connection to Ackplane's projection tables (ADR-0087).
pub struct Projector {
    client: Client,
}

impl Projector {
    /// Connect and apply the projection schema. Every statement in the
    /// migration is idempotent, matching [`crate::ledger::LedgerStore::connect`].
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane projection connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::PROJECTION,
            MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }
}

mod neighborhood;
mod rebuild;

pub use rebuild::run_projection_worker;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{DedupKey, EventEnvelope, ProvenanceClass};

    pub(super) fn structural_fact_envelope(
        key: DedupKey,
        digest: &[u8],
        fact: &StructuralFact,
    ) -> EventEnvelope {
        EventEnvelope {
            key,
            payload: serde_json::to_vec(fact).expect("encode structural fact"),
            payload_digest: digest.to_vec(),
            schema_version: "v1".to_string(),
            occurred_at: std::time::SystemTime::now(),
            payload_type: STRUCTURAL_FACT_PAYLOAD_TYPE.to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        }
    }

    #[test]
    fn a_structural_fact_round_trips_through_json() {
        let fact = StructuralFact {
            node_id: "artifact:src/lib.rs".to_string(),
            node_type: "artifact".to_string(),
            label: "src/lib.rs".to_string(),
            edges: vec![StructuralEdgeFact {
                target_id: "symbol:src/lib.rs:main".to_string(),
                relation: "contains".to_string(),
                base_weight: 1.0,
                half_life_hours: 168.0,
            }],
        };
        let encoded = serde_json::to_vec(&fact).unwrap();
        let decoded: StructuralFact = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, fact);
    }

    #[test]
    fn a_fact_with_no_edges_key_omitted_still_decodes() {
        let decoded: StructuralFact =
            serde_json::from_str(r#"{"node_id":"artifact:a","node_type":"artifact","label":"a"}"#)
                .unwrap();
        assert_eq!(decoded.edges, Vec::<StructuralEdgeFact>::new());
    }

    /// Real-database coverage. Opt-in via `ACKPLANE_TEST_DATABASE_URL`, and
    /// skipped (not failed) when it is unset (ADR-0088 clause 2). Re-exported
    /// so `rebuild::tests` and `neighborhood::tests` share this one macro
    /// rather than each declaring their own copy.
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
    pub(super) use require_test_database;

    #[test]
    fn is_deadlock_recognizes_only_the_deadlock_sqlstate() {
        assert!(is_deadlock(
            &tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED
        ));
        assert!(!is_deadlock(
            &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
        ));
        assert!(!is_deadlock(
            &tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE
        ));
    }
}
