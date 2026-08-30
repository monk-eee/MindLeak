//! Resolve this repository's *enrolled* node identity and signer from the
//! standard `MINDLEAK_ACKPLANE_*` environment variables (ADR-0100 decision 5,
//! ADR-0116).
//!
//! `ackplane-supervisor`'s `config.rs` and `lodestar-mcp`'s
//! `federation.rs` each already read this exact variable set and build a
//! signer from it independently. This is the third caller of the identical
//! pattern (`ackplane-mcp`, ADR-0137 clause 1) and, per this repository's own
//! reuse discipline ("write it twice, extract on the third"), it is the
//! shared implementation those two callers extend into rather than a fresh
//! fork. Migrating them onto this version is separate follow-up work -- see
//! `gaps.d/three-callers-independently-resolve-the-same-node-identity-env-vars.md`.
//!
//! Deliberately distinct from [`crate::identity`]'s file-based *candidate*
//! identity: that module answers "what is this repository asking to be
//! enrolled as" (used by `check_enrollment_status`, which needs no
//! `signing_key_id` because `CheckEnrollmentStatus` never resolves one).
//! This module answers "what enrolled node is this process authenticating
//! as" -- the same question `ackplane-supervisor` answers, and the one an
//! actual `NodeSync` connection challenge needs a `signing_key_id` to
//! resolve (`crates/ackplane-server/src/service/handshake.rs`).

use crate::auth::{
    decode_seed, ClaimSigner, CredentialFacilityError, CredentialFacilitySigner, SeedSigner,
};

pub const TENANT_ID_ENV: &str = "MINDLEAK_ACKPLANE_TENANT_ID";
pub const REPOSITORY_ID_ENV: &str = "MINDLEAK_ACKPLANE_REPOSITORY_ID";
pub const NODE_ID_ENV: &str = "MINDLEAK_ACKPLANE_NODE_ID";
pub const SIGNING_KEY_ID_ENV: &str = "MINDLEAK_ACKPLANE_SIGNING_KEY_ID";
/// The interim, explicit-configuration key source override (a hex-encoded
/// 32-byte Ed25519 seed). Optional: unset resolves to the OS credential
/// facility instead (ADR-0100 decision 5's default seam), matching
/// `ackplane-supervisor`/`lodestar-mcp`'s own documented posture. Never
/// logged.
pub const NODE_SIGNING_KEY_SEED_ENV: &str = "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED";
/// The service name every enrolled node's signing seed is stored under in the
/// OS credential facility, shared with `ackplane-supervisor`/`lodestar-mcp`
/// deliberately: a node enrolled once must not need to re-store the same key
/// under a second service name to run a different colocated process.
pub const CREDENTIAL_FACILITY_SERVICE: &str = "mindleak-ackplane-node-signing-key";

/// Every variable [`resolve_node_identity`] always requires, named for a
/// refusal message that says what is missing rather than only that
/// something is. `NODE_SIGNING_KEY_SEED_ENV` is a documented optional
/// override and is deliberately not listed: its absence selects the default
/// path (the OS credential facility), not an incomplete configuration.
pub const NODE_IDENTITY_ENV_VARS: &[&str] = &[
    TENANT_ID_ENV,
    REPOSITORY_ID_ENV,
    NODE_ID_ENV,
    SIGNING_KEY_ID_ENV,
];

/// Where this node's Ed25519 signing seed comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeSignerSource {
    Seed(Box<[u8; 32]>),
    CredentialFacility { service: String, account: String },
}

/// This process's declared, enrolled node identity: who it authenticates a
/// `NodeSync` connection as, not proof that the arbiter still agrees (the
/// connection challenge itself is that proof).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIdentity {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub signing_key_id: String,
    pub signer_source: NodeSignerSource,
}

impl NodeIdentity {
    /// Build the [`ClaimSigner`] this identity's source selects.
    pub fn signer(&self) -> Result<Box<dyn ClaimSigner>, CredentialFacilityError> {
        match &self.signer_source {
            NodeSignerSource::Seed(seed) => Ok(Box::new(SeedSigner::new(
                self.signing_key_id.clone(),
                self.node_id.clone(),
                seed,
            ))),
            NodeSignerSource::CredentialFacility { service, account } => {
                CredentialFacilitySigner::load(
                    self.signing_key_id.clone(),
                    self.node_id.clone(),
                    service,
                    account,
                )
                .map(|signer| Box::new(signer) as Box<dyn ClaimSigner>)
            }
        }
    }
}

/// Read this repository's enrolled node identity from the explicit
/// environment variables every colocated process already reads
/// (`ackplane-supervisor`, `lodestar-mcp`). `None` if any required variable
/// is unset, blank, or malformed -- the caller decides what that means.
pub fn resolve_node_identity<F>(environment: &F) -> Option<NodeIdentity>
where
    F: Fn(&str) -> Option<String>,
{
    let tenant_id = non_empty(environment(TENANT_ID_ENV))?;
    let repository_id = non_empty(environment(REPOSITORY_ID_ENV))?;
    let node_id = non_empty(environment(NODE_ID_ENV))?;
    let signing_key_id = non_empty(environment(SIGNING_KEY_ID_ENV))?;
    let signer_source = match non_empty(environment(NODE_SIGNING_KEY_SEED_ENV)) {
        Some(hex) => NodeSignerSource::Seed(Box::new(decode_seed(&hex)?)),
        None => NodeSignerSource::CredentialFacility {
            service: CREDENTIAL_FACILITY_SERVICE.to_string(),
            account: credential_facility_account(&tenant_id, &repository_id, &node_id),
        },
    };
    Some(NodeIdentity {
        tenant_id,
        repository_id,
        node_id,
        signing_key_id,
        signer_source,
    })
}

/// The account name a node's seed is stored under in the credential
/// facility: unique per tenant/repository/node so one shared service name
/// never collides across enrolled repositories or nodes on the same host.
fn credential_facility_account(tenant_id: &str, repository_id: &str, node_id: &str) -> String {
    format!("{tenant_id}:{repository_id}:{node_id}")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn seed_hex() -> &'static str {
        "0101010101010101010101010101010101010101010101010101010101010101"
            .get(0..64)
            .unwrap()
    }

    #[test]
    fn nothing_configured_resolves_to_nothing() {
        assert!(resolve_node_identity(&env(&[])).is_none());
    }

    #[test]
    fn a_missing_required_variable_resolves_to_nothing() {
        assert!(resolve_node_identity(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            // SIGNING_KEY_ID_ENV deliberately absent
        ]))
        .is_none());
    }

    #[test]
    fn a_blank_required_variable_is_treated_as_unset() {
        assert!(resolve_node_identity(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "   "),
        ]))
        .is_none());
    }

    #[test]
    fn a_full_declaration_with_no_seed_selects_the_credential_facility() {
        let identity = resolve_node_identity(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "signing-key-1"),
        ]))
        .expect("every required variable is set");
        assert_eq!(
            identity.signer_source,
            NodeSignerSource::CredentialFacility {
                service: CREDENTIAL_FACILITY_SERVICE.to_string(),
                account: "tenant-1:repository-1:node-1".to_string(),
            }
        );
    }

    #[test]
    fn a_declared_seed_selects_a_seed_signer() {
        let identity = resolve_node_identity(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "signing-key-1"),
            (NODE_SIGNING_KEY_SEED_ENV, seed_hex()),
        ]))
        .expect("every required variable is set");
        assert!(matches!(identity.signer_source, NodeSignerSource::Seed(_)));
        let signer = identity.signer().expect("a seed signer builds without I/O");
        assert_eq!(signer.node_id(), "node-1");
        assert_eq!(signer.signing_key_id(), "signing-key-1");
    }

    #[test]
    fn a_malformed_seed_resolves_to_nothing_rather_than_a_broken_signer() {
        assert!(resolve_node_identity(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "signing-key-1"),
            (NODE_SIGNING_KEY_SEED_ENV, "not hex"),
        ]))
        .is_none());
    }
}
