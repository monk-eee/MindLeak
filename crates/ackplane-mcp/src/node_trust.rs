//! Startup-time node-level connection trust (ADR-0137 clause 1).
//!
//! `ackplane-mcp`'s two existing tools (`check_enrollment_status`,
//! `active_claims`) each dial their own ad hoc, unauthenticated gRPC channel
//! per call -- neither authenticates *the connection itself* via `NodeSync`.
//! ADR-0137 decision 1 requires exactly that: `ackplane-mcp` proves it is
//! colocated with an already-enrolled node by completing the same
//! `Hello -> ConnectionChallenge -> ChallengeResponse -> HelloAccepted`
//! handshake `ackplane-supervisor` already performs
//! (`crates/ackplane-client/src/node_sync.rs`), using the *same*
//! `MINDLEAK_ACKPLANE_*` node identity `ackplane-supervisor` and
//! `lodestar-mcp`'s federated claim path already read
//! (`ackplane_client::node_identity`) -- no new configuration mechanism.
//!
//! **Named limitation, not a silent one:** ADR-0137 decision 1 states
//! `ackplane-mcp` "cannot run at all against a repository with no enrolled
//! node." This slice enforces that whenever an operator has *declared* a node
//! identity (`ackplane_client::node_identity::resolve_node_identity` resolves
//! `Some`): the handshake must then genuinely succeed, or this process
//! refuses to serve, mirroring `endpoint::resolve_endpoint`'s existing
//! refusal contract byte for byte. When no node identity is declared at all,
//! this slice does not yet refuse -- `check_enrollment_status` (file-based
//! candidate identity) and `active_claims` (no signer, by design) keep
//! working exactly as they do today, unregressed. Declaring node identity but
//! having it fail authentication is unambiguous and enforced today; a bare,
//! unconfigured process failing to opt in is deliberately left to a later
//! slice rather than guessed at. See
//! `gaps.d/ackplane-mcp-does-not-yet-refuse-when-no-node-identity-is-declared-at-all.md`.
//!
//! Deliberately a one-shot proof, not a held-open connection: neither
//! existing tool consumes the `NodeSync` stream itself (clause 6's concurrent-
//! connection tolerance is proven once the handshake completes, at
//! `crates/ackplane-mcp/tests/node_trust.rs`), so keeping it open would add a
//! heartbeat/reconnect lifecycle this slice has no user for yet.

use ackplane_client::node_identity::{
    resolve_node_identity, NodeIdentityError, NODE_IDENTITY_ENV_VARS,
};
use ackplane_client::NodeSyncConnection;

/// The capability this front door's one-shot handshake declares. Distinct
/// from `ackplane-supervisor`'s `"synchronize"` (which keeps the connection
/// open to carry directives): naming it separately means a future server-side
/// policy can tell the two apart if it ever needs to.
const CAPABILITY: &str = "mcp-front-door";

/// Prove this process is colocated with an enrolled node, when one is
/// declared. `Ok(())` covers both a successful proof and "nothing declared,
/// unchanged from today" (see the module doc's named limitation); only a
/// declared-but-failing identity refuses.
pub fn establish<F>(endpoint: &str, environment: &F) -> Result<(), String>
where
    F: Fn(&str) -> Option<String>,
{
    let identity = match resolve_node_identity(environment) {
        Ok(identity) => identity,
        // Nothing declared at all: the named limitation this slice kept.
        Err(NodeIdentityError::Missing(missing))
            if missing.len() == NODE_IDENTITY_ENV_VARS.len() =>
        {
            return Ok(());
        }
        // Bug fix: a *partially* declared identity, or a declared-but-malformed
        // seed, used to land in the same branch as "nothing declared" and let
        // the process serve unauthenticated. An operator who set four of the
        // five variables, or fat-fingered the seed, was told nothing and got
        // silently weaker trust than they had asked for -- the opposite of the
        // refusal this module exists to perform. Only a completely undeclared
        // identity is the documented no-op; anything half-configured is a
        // configuration error and says which part.
        Err(error) => {
            return Err(format!(
                "this node's enrolled identity is partially declared, so the front door \
                 cannot prove it is colocated with an enrolled node: {error}"
            ));
        }
    };

    let signer = identity.signer().map_err(|error| {
        format!(
            "declared an enrolled node identity ({vars}) but could not load its signing key: \
             {error}",
            vars = NODE_IDENTITY_ENV_VARS.join(", ")
        )
    })?;

    crate::tools::runtime()?
        .block_on(async {
            NodeSyncConnection::open(
                endpoint,
                signer.as_ref(),
                &identity.tenant_id,
                &identity.repository_id,
                vec![CAPABILITY.to_string()],
                0,
            )
            .await
        })
        .map(|_connection| ())
        .map_err(|error| {
            format!(
                "declared an enrolled node identity ({node_id}) but could not authenticate it \
                 with Ackplane at {endpoint}: {error}. ackplane-mcp cannot run against a \
                 repository whose declared node fails this proof (ADR-0137 clause 1).",
                node_id = identity.node_id
            )
        })
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

    fn no_env(_: &str) -> Option<String> {
        None
    }

    /// The named limitation, pinned: today's unconfigured process is
    /// unaffected by this slice.
    #[test]
    fn nothing_declared_is_not_refused() {
        assert_eq!(establish("http://127.0.0.1:8443", &no_env), Ok(()));
    }

    /// Regression: a *partially* declared identity used to take the same
    /// branch as "nothing declared" and let the process serve without ever
    /// proving it is colocated with an enrolled node. An operator who set
    /// four of the five variables got silently weaker trust than they asked
    /// for, with nothing said. Only a completely undeclared identity is the
    /// documented no-op.
    #[test]
    fn a_partially_declared_identity_is_refused_rather_than_silently_ignored() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_NODE_ID", "node-1"),
            // MINDLEAK_ACKPLANE_SIGNING_KEY_ID deliberately absent.
        ]);
        let error = establish("http://127.0.0.1:8443", &environment)
            .expect_err("a half-declared identity must not pass as 'nothing declared'");
        assert!(
            error.contains("MINDLEAK_ACKPLANE_SIGNING_KEY_ID"),
            "the refusal must name the variable still missing: {error}"
        );
    }

    /// Regression: a declared-but-malformed seed also fell into the
    /// "nothing declared" branch. The operator asked for one specific key and
    /// mistyped it; serving unauthenticated is the one response that tells
    /// them nothing.
    #[test]
    fn a_malformed_seed_is_refused_rather_than_silently_ignored() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_NODE_ID", "node-1"),
            ("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", "signing-key-1"),
            ("MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED", "not-a-seed"),
        ]);
        let error = establish("http://127.0.0.1:8443", &environment)
            .expect_err("a malformed seed must not pass as 'nothing declared'");
        assert!(
            error.contains("MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED"),
            "the refusal must name the variable that is wrong: {error}"
        );
    }

    /// A declared identity this process cannot even sign with (no seed, no
    /// credential-facility entry) is refused with the declaration named, not
    /// with an opaque connection error.
    #[test]
    fn a_declared_identity_with_no_reachable_key_is_refused_and_says_so() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_NODE_ID", "node-1"),
            ("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", "signing-key-1"),
            // No seed env var and (in a test process) no real credential-
            // facility entry for this made-up account: signer() must fail.
        ]);
        let error = establish("http://127.0.0.1:8443", &environment)
            .expect_err("no signer can be built for an account nothing ever provisioned");
        assert!(
            error.contains("MINDLEAK_ACKPLANE_TENANT_ID"),
            "the refusal must name the declaration it could not use: {error}"
        );
    }

    /// A fully declared, signable identity that the arbiter still refuses
    /// (unreachable, in this test) is refused naming the node id and ADR-0137
    /// clause 1, not a bare transport error.
    #[test]
    fn a_declared_identity_the_arbiter_cannot_reach_is_refused_and_says_why() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_NODE_ID", "node-1"),
            ("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", "signing-key-1"),
            (
                "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED",
                "0101010101010101010101010101010101010101010101010101010101010101"
                    .get(0..64)
                    .unwrap(),
            ),
        ]);
        // Port 0 on loopback never accepts a connection.
        let error =
            establish("http://127.0.0.1:0", &environment).expect_err("nothing listens on port 0");
        assert!(error.contains("node-1"), "got: {error}");
        assert!(error.contains("ADR-0137 clause 1"), "got: {error}");
    }
}
