//! Resolve the protected companion endpoint shared by local runtime processes.
//!
//! Configuration declares only a directory and scope. The companion supplies
//! public identity over IPC and owns every provider access and remote signature.

use std::path::PathBuf;

use crate::companion::{NodeClient, STATE_DIR_ENV};

pub const TENANT_ID_ENV: &str = "MINDLEAK_ACKPLANE_TENANT_ID";
pub const REPOSITORY_ID_ENV: &str = "MINDLEAK_ACKPLANE_REPOSITORY_ID";
pub const NODE_ID_ENV: &str = "MINDLEAK_ACKPLANE_NODE_ID";
pub const SIGNING_KEY_ID_ENV: &str = "MINDLEAK_ACKPLANE_SIGNING_KEY_ID";
pub const NODE_SIGNING_KEY_SEED_ENV: &str = "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED";
pub const NODE_IDENTITY_ENV_VARS: &[&str] = &[STATE_DIR_ENV, TENANT_ID_ENV, REPOSITORY_ID_ENV];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeIdentityError {
    Missing(Vec<&'static str>),
    LegacyConfiguration(Vec<&'static str>),
    InvalidStateDirectory,
}

impl std::fmt::Display for NodeIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(names) => write!(
                formatter,
                "the node companion is not configured: {} is not set. \
                 Enrol this node and start `register-me serve`, then declare its directory and scope here.",
                names.join(", ")
            ),
            Self::LegacyConfiguration(names) => write!(
                formatter,
                "unset obsolete runtime identity settings: {}. Identity and signing belong to \
                 the node companion selected by {STATE_DIR_ENV}, not this process.",
                names.join(", ")
            ),
            Self::InvalidStateDirectory => write!(formatter, "{STATE_DIR_ENV} must be an absolute directory path"),
        }
    }
}

impl std::error::Error for NodeIdentityError {}

/// Resolve public connection configuration without reading any credential facility.
pub fn resolve_node_client<F>(environment: &F) -> Result<NodeClient, NodeIdentityError>
where
    F: Fn(&str) -> Option<String>,
{
    let obsolete: Vec<_> = [
        NODE_SIGNING_KEY_SEED_ENV,
        NODE_ID_ENV,
        SIGNING_KEY_ID_ENV,
        "MINDLEAK_ACKPLANE_KEY_PATH",
    ]
    .into_iter()
    .filter(|name| environment(name).is_some())
    .collect();
    if !obsolete.is_empty() {
        return Err(NodeIdentityError::LegacyConfiguration(obsolete));
    }
    let mut missing = Vec::new();
    let mut require = |name: &'static str| match non_empty(environment(name)) {
        Some(value) => value,
        None => {
            missing.push(name);
            String::new()
        }
    };

    let directory = require(STATE_DIR_ENV);
    let tenant_id = require(TENANT_ID_ENV);
    let repository_id = require(REPOSITORY_ID_ENV);
    if !missing.is_empty() {
        return Err(NodeIdentityError::Missing(missing));
    }

    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err(NodeIdentityError::InvalidStateDirectory);
    }
    Ok(NodeClient::new(directory, tenant_id, repository_id))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
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

    #[test]
    fn nothing_configured_names_every_missing_variable() {
        assert_eq!(
            resolve_node_client(&env(&[])),
            Err(NodeIdentityError::Missing(NODE_IDENTITY_ENV_VARS.to_vec()))
        );
    }

    // Runtime seed configuration let every local process borrow the node key instead of using its owner.
    #[test]
    fn local_runtime_refuses_a_legacy_seed_even_when_identity_is_complete() {
        let result = resolve_node_client(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "signing-key-1"),
            (crate::companion::STATE_DIR_ENV, "/node-state"),
            (
                NODE_SIGNING_KEY_SEED_ENV,
                "0101010101010101010101010101010101010101010101010101010101010101",
            ),
        ]));
        assert!(
            result.is_err(),
            "runtime identity resolution must never load a private key"
        );
    }

    #[test]
    fn a_missing_required_variable_is_named_and_the_others_are_not() {
        assert_eq!(
            resolve_node_client(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
            ])),
            Err(NodeIdentityError::Missing(vec![STATE_DIR_ENV]))
        );
    }

    #[test]
    fn a_blank_required_variable_is_treated_as_unset() {
        assert_eq!(
            resolve_node_client(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (STATE_DIR_ENV, "   "),
            ])),
            Err(NodeIdentityError::Missing(vec![STATE_DIR_ENV]))
        );
    }

    #[test]
    fn a_malformed_seed_is_reported_as_obsolete_not_missing() {
        assert_eq!(
            resolve_node_client(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (NODE_SIGNING_KEY_SEED_ENV, "not-hex"),
            ])),
            Err(NodeIdentityError::LegacyConfiguration(vec![
                NODE_SIGNING_KEY_SEED_ENV
            ]))
        );
    }

    #[test]
    fn an_obsolete_variable_never_turns_into_an_unconfigured_noop() {
        assert_eq!(
            resolve_node_client(&env(&[(NODE_SIGNING_KEY_SEED_ENV, "")])),
            Err(NodeIdentityError::LegacyConfiguration(vec![
                NODE_SIGNING_KEY_SEED_ENV
            ]))
        );
    }

    #[test]
    fn a_full_declaration_selects_only_the_companion() {
        let directory = std::env::temp_dir().join("node-state");
        let client = resolve_node_client(&env(&[
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (STATE_DIR_ENV, directory.to_str().unwrap()),
        ]))
        .expect("every required variable is set");
        assert_eq!(client.state_dir, directory);
        assert_eq!(client.tenant_id, "tenant-1");
        assert_eq!(client.repository_id, "repository-1");
    }

    #[test]
    fn a_relative_directory_is_refused() {
        assert_eq!(
            resolve_node_client(&env(&[
                (TENANT_ID_ENV, "tenant-1"),
                (REPOSITORY_ID_ENV, "repository-1"),
                (STATE_DIR_ENV, "relative/node-state"),
            ])),
            Err(NodeIdentityError::InvalidStateDirectory)
        );
    }
}
