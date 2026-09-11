//! Startup proof through the provider-owning companion (ADR-0100/0137).

use ackplane_client::resolve_node_client;

/// Require an enrolled companion; no configuration never means authenticated.
pub fn establish<F>(endpoint: &str, environment: &F) -> Result<(), String>
where
    F: Fn(&str) -> Option<String>,
{
    let mut node = resolve_node_client(environment).map_err(|error| error.to_string())?;
    node.expected_endpoint = Some(endpoint.to_string());

    crate::tools::runtime()?
        .block_on(async { node.open_sync(0, None).await })
        .map(|_connection| ())
        .map_err(|error| {
            format!(
                "the node companion could not prove enrolled authority: {error}. \
                 ackplane-mcp requires a running companion for {endpoint} (ADR-0137 clause 1)."
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
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

    /// An undeclared node used to bypass startup trust completely.
    #[test]
    fn nothing_declared_is_refused() {
        assert!(establish("http://127.0.0.1:8443", &no_env).is_err());
    }

    /// Missing companion configuration must never turn into unauthenticated startup.
    #[test]
    fn a_partially_declared_identity_is_refused_rather_than_silently_ignored() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
        ]);
        let error = establish("http://127.0.0.1:8443", &environment)
            .expect_err("a half-declared identity must not pass as 'nothing declared'");
        assert!(
            error.contains("MINDLEAK_ACKPLANE_STATE_DIR"),
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

    /// Legacy identity settings must not trigger a new credential reader.
    #[test]
    fn a_declared_identity_with_no_reachable_key_is_refused_and_says_so() {
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_NODE_ID", "node-1"),
            ("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", "signing-key-1"),
        ]);
        let error = establish("http://127.0.0.1:8443", &environment)
            .expect_err("no signer can be built for an account nothing ever provisioned");
        assert!(
            error.contains("MINDLEAK_ACKPLANE_SIGNING_KEY_ID"),
            "the refusal must name the declaration it could not use: {error}"
        );
    }

    /// Reachability cannot be inferred from a syntactically complete configuration.
    #[test]
    fn an_unreachable_companion_is_refused_and_says_why() {
        let directory = std::env::temp_dir().join("missing-ackplane-node-companion");
        let environment = env(&[
            ("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-1"),
            ("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-1"),
            ("MINDLEAK_ACKPLANE_STATE_DIR", directory.to_str().unwrap()),
        ]);
        // Port 0 on loopback never accepts a connection.
        let error =
            establish("http://127.0.0.1:0", &environment).expect_err("nothing listens on port 0");
        assert!(error.contains("companion"), "got: {error}");
        assert!(error.contains("ADR-0137 clause 1"), "got: {error}");
    }
}
