//! The exact bytes a repository node signs to authenticate a KnowledgeService
//! request (ADR-0108), mirroring `claim_auth`'s shape with its own domain and
//! operation fields -- a knowledge statement has no branch, lease, or scope to
//! bind, so reusing `ClaimOperation`/`CLAIM_DOMAIN` would sign nothing
//! meaningful for this domain.

use crate::signing_bytes::push_field;
use crate::v1;

/// Domain separation for knowledge-request signatures. Distinct from
/// `claim_auth::CLAIM_DOMAIN` so a signature produced for one domain can
/// never verify as the other, even if every other field happened to coincide.
pub const KNOWLEDGE_DOMAIN: &[u8] = b"mindleak.ackplane.v1.knowledge\0";

/// Which `KnowledgeService` RPC an authentication authorizes, and that
/// operation's own fields.
///
/// Identity alone (tenant/repository/node/key) proves *who* is asking, not
/// *what* they asked for: binding the operation tag and every
/// operation-specific field closes that gap, the same reasoning ADR-0100
/// decision 10 already applied to claims.
#[derive(Debug, Clone, Copy)]
pub enum KnowledgeOperation<'a> {
    Record {
        content: &'a str,
        source_ref: Option<&'a str>,
        reach_node_ids: &'a [String],
        reach_goal_id: Option<&'a str>,
        half_life_hours: f64,
        embedding_model: Option<&'a str>,
    },
    Recall {
        /// Whether a query embedding was supplied, not the vector itself --
        /// a read RPC's authentication binds what the caller is asking to be
        /// shown under, not the (potentially large) ranking input verbatim.
        query_embedding_present: bool,
        limit: u32,
    },
    History {
        limit: u32,
    },
    Reconfirm {
        knowledge_id: &'a str,
        evidence_ref: &'a str,
    },
    Retire {
        knowledge_id: &'a str,
        reason: &'a str,
    },
}

impl KnowledgeOperation<'_> {
    /// The distinct tag every variant signs, so a signature for one RPC can
    /// never verify for another even when every other field happens to
    /// coincide.
    fn tag(&self) -> &'static str {
        match self {
            Self::Record { .. } => "record",
            Self::Recall { .. } => "recall",
            Self::History { .. } => "history",
            Self::Reconfirm { .. } => "reconfirm",
            Self::Retire { .. } => "retire",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        match self {
            Self::Record {
                content,
                source_ref,
                reach_node_ids,
                reach_goal_id,
                half_life_hours,
                embedding_model,
            } => {
                push_field(bytes, content.as_bytes());
                push_field(bytes, source_ref.unwrap_or("").as_bytes());
                let reach_count = u64::try_from(reach_node_ids.len()).unwrap_or(u64::MAX);
                push_field(bytes, &reach_count.to_be_bytes());
                for reach_node_id in *reach_node_ids {
                    push_field(bytes, reach_node_id.as_bytes());
                }
                push_field(bytes, reach_goal_id.unwrap_or("").as_bytes());
                push_field(bytes, &half_life_hours.to_be_bytes());
                push_field(bytes, embedding_model.unwrap_or("").as_bytes());
            }
            Self::Recall {
                query_embedding_present,
                limit,
            } => {
                push_field(bytes, &[*query_embedding_present as u8]);
                push_field(bytes, &limit.to_be_bytes());
            }
            Self::History { limit } => {
                push_field(bytes, &limit.to_be_bytes());
            }
            Self::Reconfirm {
                knowledge_id,
                evidence_ref,
            } => {
                push_field(bytes, knowledge_id.as_bytes());
                push_field(bytes, evidence_ref.as_bytes());
            }
            Self::Retire {
                knowledge_id,
                reason,
            } => {
                push_field(bytes, knowledge_id.as_bytes());
                push_field(bytes, reason.as_bytes());
            }
        }
    }
}

/// Binds the authentication to this specific request's identity -- tenant and
/// repository -- and to the exact operation and fields being authorized.
/// Every field is length-delimited, following `claim_auth::claim_signing_bytes`.
pub fn knowledge_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &KnowledgeOperation,
    authentication: &v1::KnowledgeAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 6] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(KNOWLEDGE_DOMAIN.len() + 64);
    bytes.extend_from_slice(KNOWLEDGE_DOMAIN);
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication(nonce: u8) -> v1::KnowledgeAuthentication {
        v1::KnowledgeAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            nonce: vec![nonce; 16],
            signature: Vec::new(),
        }
    }

    const RECORD: KnowledgeOperation<'static> = KnowledgeOperation::Record {
        content: "a lesson",
        source_ref: Some("pr:538"),
        reach_node_ids: &[],
        reach_goal_id: None,
        half_life_hours: 720.0,
        embedding_model: Some("model-a"),
    };

    #[test]
    fn identical_inputs_produce_identical_bytes() {
        let a = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let b = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        assert_eq!(a, b);
    }

    #[test]
    fn a_different_operation_tag_changes_the_bytes() {
        let record = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let retire_bytes = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Retire {
                knowledge_id: "know-1",
                reason: "superseded",
            },
            &authentication(1),
        );
        assert_ne!(record, retire_bytes);
    }

    #[test]
    fn a_different_nonce_changes_the_bytes() {
        let a = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let b = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(2));
        assert_ne!(a, b);
    }

    #[test]
    fn a_changed_operation_field_changes_the_bytes() {
        let a = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let changed = KnowledgeOperation::Record {
            content: "a different lesson",
            source_ref: Some("pr:538"),
            reach_node_ids: &[],
            reach_goal_id: None,
            half_life_hours: 720.0,
            embedding_model: Some("model-a"),
        };
        let b = knowledge_signing_bytes("tenant-a", "repo-a", &changed, &authentication(1));
        assert_ne!(a, b);
    }

    #[test]
    fn a_changed_source_reference_changes_record_bytes() {
        let a = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let changed = KnowledgeOperation::Record {
            content: "a lesson",
            source_ref: Some("pr:539"),
            reach_node_ids: &[],
            reach_goal_id: None,
            half_life_hours: 720.0,
            embedding_model: Some("model-a"),
        };
        let b = knowledge_signing_bytes("tenant-a", "repo-a", &changed, &authentication(1));

        assert_ne!(a, b);
    }

    #[test]
    fn changed_reach_metadata_changes_record_bytes() {
        let reach_node_ids = vec!["artifact:crates/ackplane-server/src/fleet.rs".to_string()];
        let original = KnowledgeOperation::Record {
            content: "a lesson",
            source_ref: Some("pr:538"),
            reach_node_ids: &reach_node_ids,
            reach_goal_id: Some("goal:ackplane-federation-service"),
            half_life_hours: 720.0,
            embedding_model: Some("model-a"),
        };
        let changed_goal = KnowledgeOperation::Record {
            content: "a lesson",
            source_ref: Some("pr:538"),
            reach_node_ids: &reach_node_ids,
            reach_goal_id: Some("goal:other"),
            half_life_hours: 720.0,
            embedding_model: Some("model-a"),
        };
        let changed_nodes = KnowledgeOperation::Record {
            content: "a lesson",
            source_ref: Some("pr:538"),
            reach_node_ids: &[],
            reach_goal_id: Some("goal:ackplane-federation-service"),
            half_life_hours: 720.0,
            embedding_model: Some("model-a"),
        };

        let original = knowledge_signing_bytes("tenant-a", "repo-a", &original, &authentication(1));
        let changed_goal =
            knowledge_signing_bytes("tenant-a", "repo-a", &changed_goal, &authentication(1));
        let changed_nodes =
            knowledge_signing_bytes("tenant-a", "repo-a", &changed_nodes, &authentication(1));

        assert_ne!(original, changed_goal);
        assert_ne!(original, changed_nodes);
    }

    #[test]
    fn recall_binds_whether_a_query_embedding_was_supplied_and_the_limit() {
        let with_embedding = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Recall {
                query_embedding_present: true,
                limit: 10,
            },
            &authentication(1),
        );
        let without_embedding = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Recall {
                query_embedding_present: false,
                limit: 10,
            },
            &authentication(1),
        );
        let different_limit = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Recall {
                query_embedding_present: true,
                limit: 20,
            },
            &authentication(1),
        );
        assert_ne!(with_embedding, without_embedding);
        assert_ne!(with_embedding, different_limit);
    }

    #[test]
    fn history_binds_its_limit_and_never_shares_a_recall_signature() {
        let history_ten = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::History { limit: 10 },
            &authentication(1),
        );
        let history_twenty = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::History { limit: 20 },
            &authentication(1),
        );
        let recall_ten = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Recall {
                query_embedding_present: false,
                limit: 10,
            },
            &authentication(1),
        );

        assert_ne!(history_ten, history_twenty);
        assert_ne!(history_ten, recall_ten);
    }

    #[test]
    fn reconfirmation_binds_its_knowledge_and_corroborating_evidence() {
        let original = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Reconfirm {
                knowledge_id: "knowledge-1",
                evidence_ref: "evidence:verified",
            },
            &authentication(1),
        );
        let different_evidence = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Reconfirm {
                knowledge_id: "knowledge-1",
                evidence_ref: "evidence:other",
            },
            &authentication(1),
        );
        let different_knowledge = knowledge_signing_bytes(
            "tenant-a",
            "repo-a",
            &KnowledgeOperation::Reconfirm {
                knowledge_id: "knowledge-2",
                evidence_ref: "evidence:verified",
            },
            &authentication(1),
        );

        assert_ne!(original, different_evidence);
        assert_ne!(original, different_knowledge);
    }

    #[test]
    fn a_knowledge_signature_never_shares_bytes_with_a_claim_domain_separator() {
        let bytes = knowledge_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        assert!(bytes.starts_with(KNOWLEDGE_DOMAIN));
        assert_ne!(KNOWLEDGE_DOMAIN, crate::claim_auth::CLAIM_DOMAIN);
    }
}
