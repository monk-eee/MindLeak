//! Configuration resolution for the supervisor daemon.
//!
//! Pure and environment-injected: no process globals, no file reads, no
//! network. The daemon supplies a closure over the real environment; tests
//! supply a map. That keeps every refusal path — which is most of this module —
//! exhaustively testable without a machine that happens to be misconfigured.
//!
//! Every variable here already exists. `MINDLEAK_ACKPLANE_ENDPOINT`,
//! `_TENANT_ID`, `_REPOSITORY_ID`, `_NODE_ID`, `_SIGNING_KEY_ID`,
//! `_NODE_SIGNING_KEY_SEED` and `MINDLEAK_ACKPLANE_KEY_PATH` are the same names
//! `lodestar-mcp`'s federated claim path and `register-me` already read, so an
//! operator who has enrolled a node has already configured this daemon
//! (ADR-0116; no new configuration mechanism is invented here).

use std::{path::PathBuf, time::Duration};

/// The OS credential facility service every enrolled node's seed is stored
/// under. Shared with `lodestar-mcp`'s federated claim path deliberately: two
/// service names would mean a node enrolled for claims could not be run as a
/// supervisor without re-storing the same key.
pub const CREDENTIAL_FACILITY_SERVICE: &str = "mindleak-ackplane-node-signing-key";

const TENANT_ID_ENV: &str = "MINDLEAK_ACKPLANE_TENANT_ID";
const REPOSITORY_ID_ENV: &str = "MINDLEAK_ACKPLANE_REPOSITORY_ID";
const NODE_ID_ENV: &str = "MINDLEAK_ACKPLANE_NODE_ID";
const SIGNING_KEY_ID_ENV: &str = "MINDLEAK_ACKPLANE_SIGNING_KEY_ID";
const NODE_SIGNING_KEY_SEED_ENV: &str = "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED";
const ENDPOINT_ENV: &str = "MINDLEAK_ACKPLANE_ENDPOINT";

/// This supervisor's own id. Distinct from the node id: one enrolled node may
/// run several supervisors, and a directive is addressed to a supervisor's
/// session rather than to the node.
const SUPERVISOR_ID_ENV: &str = "ACKPLANE_SUPERVISOR_ID";
/// Where the durable inbox and outbox live. A supervisor that cannot persist
/// its receipts has no business claiming it processed anything.
const STATE_DIR_ENV: &str = "ACKPLANE_SUPERVISOR_STATE_DIR";
const HEARTBEAT_SECONDS_ENV: &str = "ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS";

const DEFAULT_STATE_DIR: &str = ".mindleak/supervisor";
const DEFAULT_HEARTBEAT_SECONDS: u64 = 30;

/// Where this supervisor's signing key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerSource {
    /// An explicit hex seed. Non-hardened and selected only when
    /// `MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED` is set, matching the interim
    /// posture `lodestar-mcp` already documents for the same variable.
    Seed(Box<[u8; 32]>),
    /// The OS credential facility, keyed per node so one service name holds
    /// every enrolled node's seed without collision.
    CredentialFacility { service: String, account: String },
}

/// Everything the daemon needs to run, once resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    pub endpoint: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub signing_key_id: String,
    pub supervisor_id: String,
    pub signer_source: SignerSource,
    pub state_dir: PathBuf,
    pub heartbeat_interval: Duration,
}

impl SupervisorConfig {
    /// The durable inbox path for this supervisor.
    pub fn inbox_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("{}.inbox.db", self.supervisor_id))
    }

    /// The durable outbox path for this supervisor.
    pub fn outbox_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("{}.outbox.db", self.supervisor_id))
    }
}

/// Why configuration could not be resolved.
///
/// Naming the missing variables rather than reporting "misconfigured" is the
/// whole point: an operator reading a refusal should not have to diff their
/// environment against a doc page to find the one they forgot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Missing(Vec<&'static str>),
    MalformedSeed,
    MalformedHeartbeat(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(names) => write!(
                formatter,
                "the supervisor cannot start: {} is not set. Enrol this node first \
                 (`register-me`), then declare it here.",
                names.join(", ")
            ),
            Self::MalformedSeed => write!(
                formatter,
                "{NODE_SIGNING_KEY_SEED_ENV} must be 64 hex characters (a 32-byte Ed25519 \
                 seed). Unset it to use the OS credential facility instead."
            ),
            Self::MalformedHeartbeat(value) => write!(
                formatter,
                "{HEARTBEAT_SECONDS_ENV} must be a positive whole number of seconds, not {value:?}"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Resolve the daemon's configuration from `environment`.
///
/// Every required variable is reported together rather than one per run: an
/// operator configuring a new node otherwise learns about the next missing
/// variable only after fixing the previous one.
pub fn resolve<F>(environment: F) -> Result<SupervisorConfig, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let read = |name: &str| {
        environment(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    let mut missing = Vec::new();
    let mut require = |name: &'static str| match read(name) {
        Some(value) => value,
        None => {
            missing.push(name);
            String::new()
        }
    };

    let endpoint = require(ENDPOINT_ENV);
    let tenant_id = require(TENANT_ID_ENV);
    let repository_id = require(REPOSITORY_ID_ENV);
    let node_id = require(NODE_ID_ENV);
    let signing_key_id = require(SIGNING_KEY_ID_ENV);
    let supervisor_id = require(SUPERVISOR_ID_ENV);
    if !missing.is_empty() {
        return Err(ConfigError::Missing(missing));
    }

    let signer_source = match read(NODE_SIGNING_KEY_SEED_ENV) {
        Some(hex) => SignerSource::Seed(Box::new(
            decode_seed(&hex).ok_or(ConfigError::MalformedSeed)?,
        )),
        None => SignerSource::CredentialFacility {
            service: CREDENTIAL_FACILITY_SERVICE.to_string(),
            account: format!("{tenant_id}:{repository_id}:{node_id}"),
        },
    };

    let heartbeat_interval = match read(HEARTBEAT_SECONDS_ENV) {
        Some(value) => {
            let seconds: u64 = value
                .parse()
                .map_err(|_| ConfigError::MalformedHeartbeat(value.clone()))?;
            if seconds == 0 {
                return Err(ConfigError::MalformedHeartbeat(value));
            }
            Duration::from_secs(seconds)
        }
        None => Duration::from_secs(DEFAULT_HEARTBEAT_SECONDS),
    };

    Ok(SupervisorConfig {
        endpoint,
        tenant_id,
        repository_id,
        node_id,
        signing_key_id,
        supervisor_id,
        signer_source,
        state_dir: read(STATE_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR)),
        heartbeat_interval,
    })
}

/// Decode a 64-character hex seed. Kept local rather than reaching into
/// `ackplane-client` so this module stays free of the network stack it would
/// otherwise pull in for a pure string function.
fn decode_seed(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut seed = [0_u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    fn complete() -> Vec<(&'static str, &'static str)> {
        vec![
            (ENDPOINT_ENV, "http://127.0.0.1:8443"),
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (NODE_ID_ENV, "node-1"),
            (SIGNING_KEY_ID_ENV, "signing-key-1"),
            (SUPERVISOR_ID_ENV, "supervisor-1"),
        ]
    }

    #[test]
    fn a_complete_environment_resolves_with_the_credential_facility_by_default() {
        let config = resolve(environment(&complete())).expect("configuration should resolve");

        assert_eq!(config.endpoint, "http://127.0.0.1:8443");
        assert_eq!(config.supervisor_id, "supervisor-1");
        assert_eq!(
            config.signer_source,
            SignerSource::CredentialFacility {
                service: CREDENTIAL_FACILITY_SERVICE.to_string(),
                account: "tenant-1:repository-1:node-1".to_string(),
            },
            "an unset seed selects the hardened path, not a failure"
        );
        assert_eq!(config.heartbeat_interval, Duration::from_secs(30));
        assert_eq!(config.state_dir, PathBuf::from(DEFAULT_STATE_DIR));
    }

    /// An operator configuring a new node should learn about every missing
    /// variable at once. Reporting them one per run turns a single mistake
    /// into as many restarts as there are unset variables.
    #[test]
    fn every_missing_variable_is_reported_together() {
        let error = resolve(environment(&[(ENDPOINT_ENV, "http://127.0.0.1:8443")]))
            .expect_err("an incomplete environment must be refused");

        let ConfigError::Missing(names) = &error else {
            panic!("expected a missing-variable refusal, got {error:?}");
        };
        assert_eq!(
            names,
            &vec![
                TENANT_ID_ENV,
                REPOSITORY_ID_ENV,
                NODE_ID_ENV,
                SIGNING_KEY_ID_ENV,
                SUPERVISOR_ID_ENV
            ]
        );
        let message = error.to_string();
        assert!(
            message.contains(NODE_ID_ENV),
            "the refusal must name the variables: {message}"
        );
    }

    /// A variable set to whitespace is a configuration mistake that looks
    /// exactly like a correct one in a shell script, so it is treated as unset
    /// rather than accepted as a blank identity.
    #[test]
    fn a_blank_variable_counts_as_missing() {
        let mut pairs = complete();
        pairs.retain(|(name, _)| *name != NODE_ID_ENV);
        pairs.push((NODE_ID_ENV, "   "));

        let error = resolve(environment(&pairs)).expect_err("a blank node id must be refused");

        assert_eq!(error, ConfigError::Missing(vec![NODE_ID_ENV]));
    }

    #[test]
    fn an_explicit_seed_overrides_the_credential_facility() {
        let mut pairs = complete();
        let seed = "ab".repeat(32);
        pairs.push((NODE_SIGNING_KEY_SEED_ENV, seed.as_str()));

        let config = resolve(environment(&pairs)).expect("configuration should resolve");

        assert_eq!(
            config.signer_source,
            SignerSource::Seed(Box::new([0xab; 32]))
        );
    }

    /// A truncated or mistyped seed must not silently fall back to the
    /// credential facility: the operator asked for a specific key, and quietly
    /// using a different one is worse than refusing.
    #[test]
    fn a_malformed_seed_is_refused_rather_than_falling_back() {
        let mut pairs = complete();
        pairs.push((NODE_SIGNING_KEY_SEED_ENV, "not-a-seed"));

        let error = resolve(environment(&pairs)).expect_err("a malformed seed must be refused");

        assert_eq!(error, ConfigError::MalformedSeed);
    }

    #[test]
    fn a_zero_heartbeat_is_refused() {
        let mut pairs = complete();
        pairs.push((HEARTBEAT_SECONDS_ENV, "0"));

        let error = resolve(environment(&pairs)).expect_err("a zero heartbeat must be refused");

        assert!(matches!(error, ConfigError::MalformedHeartbeat(_)));
    }

    #[test]
    fn state_paths_are_scoped_to_the_supervisor() {
        let mut pairs = complete();
        pairs.push((STATE_DIR_ENV, "/var/lib/ackplane"));
        let config = resolve(environment(&pairs)).expect("configuration should resolve");

        assert!(config.inbox_path().ends_with("supervisor-1.inbox.db"));
        assert!(config.outbox_path().ends_with("supervisor-1.outbox.db"));
        assert_ne!(config.inbox_path(), config.outbox_path());
    }
}
