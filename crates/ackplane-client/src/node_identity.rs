//! Resolve this repository's *enrolled* node identity and signer from the
//! standard `MINDLEAK_ACKPLANE_*` environment variables (ADR-0100 decision 5,
//! ADR-0116).
//!
//! `ackplane-supervisor`'s `config.rs`, `lodestar-mcp`'s `federation.rs` and
//! `ackplane-mcp`'s `node_trust.rs` all resolve this repository's enrolled
//! node identity from the same variables. This module is the one
//! implementation they share, so a change to the credential-facility account
//! scheme, the service name, or the seed encoding reaches every process that
//! authenticates as this node instead of one of them.
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

/// Why a node identity could not be resolved.
///
/// A bare `None` forced every caller that wanted a useful refusal to
/// reconstruct the reason itself, which is exactly why `ackplane-supervisor`
/// grew a parallel resolver with its own missing-variable tracking and
/// `ackplane-mcp` settled for listing *every* variable rather than the ones
/// actually absent. Naming the cause here is what lets both drop their copies
/// without either losing the message quality it already had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeIdentityError {
    /// Required variables that are unset or blank. Reported together rather
    /// than one per run: an operator configuring a new node otherwise learns
    /// about the next missing variable only after fixing the previous one.
    Missing(Vec<&'static str>),
    /// `NODE_SIGNING_KEY_SEED_ENV` was set but is not a 64-character hex
    /// encoding of a 32-byte Ed25519 seed. Distinct from `Missing`: the
    /// operator configured the override and got it wrong, which is a
    /// different fix from not configuring it at all.
    MalformedSeed,
}

impl std::fmt::Display for NodeIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(names) => write!(
                formatter,
                "this node's enrolled identity is not declared: {} is not set. \
                 Enrol this node first (`register-me`), then declare it here.",
                names.join(", ")
            ),
            Self::MalformedSeed => write!(
                formatter,
                "{NODE_SIGNING_KEY_SEED_ENV} must be 64 hex characters (a 32-byte Ed25519 \
                 seed). Unset it to use the OS credential facility instead."
            ),
        }
    }
}

impl std::error::Error for NodeIdentityError {}

/// Read this repository's enrolled node identity from the explicit
/// environment variables every colocated process already reads
/// (`ackplane-supervisor`, `lodestar-mcp`, `ackplane-mcp`).
///
/// Every missing required variable is reported together, so a caller can
/// name all of them in one refusal. The caller still decides what a failure
/// *means* -- refusing to serve, running unfederated, or reporting a
/// configuration error -- this only decides what the environment says.
pub fn resolve_node_identity<F>(environment: &F) -> Result<NodeIdentity, NodeIdentityError>
where
    F: Fn(&str) -> Option<String>,
{
    let mut missing = Vec::new();
    let mut require = |name: &'static str| match non_empty(environment(name)) {
        Some(value) => value,
        None => {
            missing.push(name);
            String::new()
        }
    };

    let tenant_id = require(TENANT_ID_ENV);
    let repository_id = require(REPOSITORY_ID_ENV);
    let node_id = require(NODE_ID_ENV);
    let signing_key_id = require(SIGNING_KEY_ID_ENV);
    if !missing.is_empty() {
        return Err(NodeIdentityError::Missing(missing));
    }

    let signer_source = match non_empty(environment(NODE_SIGNING_KEY_SEED_ENV)) {
        Some(hex) => NodeSignerSource::Seed(Box::new(
            decode_seed(&hex).ok_or(NodeIdentityError::MalformedSeed)?,
        )),
        None => NodeSignerSource::CredentialFacility {
            service: CREDENTIAL_FACILITY_SERVICE.to_string(),
            account: credential_facility_account(&tenant_id, &repository_id, &node_id),
        },
    };
    Ok(NodeIdentity {
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
    fn nothing_configured_names_every_missing_variable() {
        // Regression: this returned a bare `None`, so a caller wanting to
        // tell the operator what to fix had to re-derive the list itself --
        // which is how the supervisor ended up with a parallel resolver.
        assert_eq!(
            resolve_node_identity(&env(&[])),
            Err(NodeIdentityError::Missing(NODE_IDENTITY_ENV_VARS.to_vec()))
        );
    }

    #[test]
    fn a_missing_required_variable_is_named_and_the_others_are_not() {
        assert_eq!(
            resolve_node_identity(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (NODE_ID_ENV, "node-1"),
                // SIGNING_KEY_ID_ENV deliberately absent
            ])),
            Err(NodeIdentityError::Missing(vec![SIGNING_KEY_ID_ENV]))
        );
    }

    #[test]
    fn a_blank_required_variable_is_treated_as_unset() {
        assert_eq!(
            resolve_node_identity(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (NODE_ID_ENV, "node-1"),
                (SIGNING_KEY_ID_ENV, "   "),
            ])),
            Err(NodeIdentityError::Missing(vec![SIGNING_KEY_ID_ENV]))
        );
    }

    /// A malformed seed is a different operator fix from an undeclared one,
    /// so it must not be collapsed into "something is missing".
    #[test]
    fn a_malformed_seed_is_reported_as_malformed_not_missing() {
        assert_eq!(
            resolve_node_identity(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (NODE_ID_ENV, "node-1"),
                (SIGNING_KEY_ID_ENV, "signing-key-1"),
                (NODE_SIGNING_KEY_SEED_ENV, "not-hex"),
            ])),
            Err(NodeIdentityError::MalformedSeed)
        );
    }

    /// An absent required variable is reported before a malformed seed: an
    /// operator who has not finished declaring the identity is not yet in a
    /// position to act on a complaint about the optional override.
    #[test]
    fn a_missing_variable_is_reported_ahead_of_a_malformed_seed() {
        assert_eq!(
            resolve_node_identity(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (NODE_ID_ENV, "node-1"),
                (NODE_SIGNING_KEY_SEED_ENV, "not-hex"),
            ])),
            Err(NodeIdentityError::Missing(vec![SIGNING_KEY_ID_ENV]))
        );
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
}
