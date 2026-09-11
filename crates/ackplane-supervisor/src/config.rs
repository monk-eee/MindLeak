//! Configuration resolution for the supervisor daemon.
//!
//! Pure and environment-injected: no process globals, no file reads, no
//! network. The daemon supplies a closure over the real environment; tests
//! supply a map. That keeps every refusal path — which is most of this module —
//! exhaustively testable without a machine that happens to be misconfigured.
//!
//! Shared enrollment configuration names only the companion state directory,
//! tenant, and repository. Named workers remain explicit local configuration.
//!
//! The enrolled node half of that set is resolved by
//! [`ackplane_client::node_identity`], not re-implemented here. This module
//! owns only what is genuinely the supervisor's: its own
//! supervisor id, its state directory and its heartbeat interval.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use crate::WorkerCommand;
use ackplane_client::{
    companion::NodeClient,
    node_identity::{resolve_node_client, NodeIdentityError},
};

/// This supervisor's own id. Distinct from the node id: one enrolled node may
/// run several supervisors, and a directive is addressed to a supervisor's
/// session rather than to the node.
const SUPERVISOR_ID_ENV: &str = "ACKPLANE_SUPERVISOR_ID";
/// Where the durable inbox and outbox live. A supervisor that cannot persist
/// its receipts has no business claiming it processed anything.
const STATE_DIR_ENV: &str = "ACKPLANE_SUPERVISOR_STATE_DIR";
const HEARTBEAT_SECONDS_ENV: &str = "ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS";
pub const WORKERS_ENV: &str = "ACKPLANE_SUPERVISOR_WORKERS";

const DEFAULT_STATE_DIR: &str = ".mindleak/supervisor";
const DEFAULT_HEARTBEAT_SECONDS: u64 = 30;

/// Everything the daemon needs to run, once resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    pub node: NodeClient,
    pub supervisor_id: String,
    pub state_dir: PathBuf,
    pub heartbeat_interval: Duration,
    pub workers: BTreeMap<String, WorkerCommand>,
}

impl SupervisorConfig {
    pub fn worker_run_path(&self, name: &str) -> PathBuf {
        self.state_dir.join(format!("{name}.worker-run.json"))
    }

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
    Identity(NodeIdentityError),
    MalformedHeartbeat(String),
    InvalidSupervisorId,
    InvalidWorkers(String),
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
            Self::Identity(error) => write!(formatter, "{error}"),
            Self::InvalidSupervisorId => write!(
                formatter,
                "{SUPERVISOR_ID_ENV} must contain 1-64 letters, digits, hyphens or underscores"
            ),
            Self::InvalidWorkers(reason) => write!(formatter, "{WORKERS_ENV}: {reason}"),
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
/// variable only after fixing the previous one. That includes the enrolled
/// node's variables, which the shared resolver reports by name for exactly
/// this reason.
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

    let node = resolve_node_client(&environment);
    if let Err(NodeIdentityError::Missing(names)) = &node {
        missing.extend(names.iter().copied());
    }
    let supervisor_id = read(SUPERVISOR_ID_ENV);
    if supervisor_id.is_none() {
        missing.push(SUPERVISOR_ID_ENV);
    }
    if !missing.is_empty() {
        return Err(ConfigError::Missing(missing));
    }
    let node = node.map_err(ConfigError::Identity)?;
    let supervisor_id = supervisor_id.expect("supervisor id is present once nothing is missing");
    let valid_local_name = |value: &str| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    };
    if !valid_local_name(&supervisor_id) {
        return Err(ConfigError::InvalidSupervisorId);
    }

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

    let workers: BTreeMap<String, WorkerCommand> = match read(WORKERS_ENV) {
        Some(json) => serde_json::from_str(&json)
            .map_err(|error| ConfigError::InvalidWorkers(error.to_string()))?,
        None => BTreeMap::new(),
    };
    if workers.len() > 32 {
        return Err(ConfigError::InvalidWorkers(
            "at most 32 named workers are supported".into(),
        ));
    }
    let mut directories = std::collections::HashSet::new();
    for (name, worker) in &workers {
        if !valid_local_name(name) {
            return Err(ConfigError::InvalidWorkers(
                "worker names must contain 1-64 letters, digits, hyphens or underscores".into(),
            ));
        }
        worker
            .validate()
            .map_err(|error| ConfigError::InvalidWorkers(error.to_string()))?;
        if !directories.insert(&worker.working_directory) {
            return Err(ConfigError::InvalidWorkers(
                "workers must have separate working directories".into(),
            ));
        }
    }

    Ok(SupervisorConfig {
        node,
        supervisor_id,
        state_dir: read(STATE_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR)),
        heartbeat_interval,
        workers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ackplane_client::companion::STATE_DIR_ENV as NODE_STATE_DIR_ENV;
    use ackplane_client::node_identity::{
        NODE_SIGNING_KEY_SEED_ENV, REPOSITORY_ID_ENV, TENANT_ID_ENV,
    };
    use std::collections::HashMap;

    fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    fn complete() -> Vec<(&'static str, &'static str)> {
        #[cfg(windows)]
        let directory = "C:\\node-state";
        #[cfg(unix)]
        let directory = "/node-state";
        vec![
            (NODE_STATE_DIR_ENV, directory),
            (TENANT_ID_ENV, "tenant-1"),
            (REPOSITORY_ID_ENV, "repository-1"),
            (SUPERVISOR_ID_ENV, "supervisor-1"),
        ]
    }

    #[test]
    fn a_complete_environment_resolves_only_the_companion() {
        let config = resolve(environment(&complete())).expect("configuration should resolve");

        assert!(config.node.state_dir.is_absolute());
        assert_eq!(config.supervisor_id, "supervisor-1");
        assert_eq!(config.node.tenant_id, "tenant-1");
        assert_eq!(config.heartbeat_interval, Duration::from_secs(30));
        assert_eq!(config.state_dir, PathBuf::from(DEFAULT_STATE_DIR));
        assert!(config.workers.is_empty());
    }

    #[test]
    fn multiple_runtime_commands_are_configured_without_a_shell() {
        let root = std::env::temp_dir();
        let workers = serde_json::json!({
            "copilot": {"command":"copilot", "args":["-p","{prompt}"], "working_directory":root.join("worker-first"), "branch":"agents/first"},
            "claude": {"command":"claude", "args":["-p","{prompt}"], "working_directory":root.join("worker-second"), "branch":"agents/second"},
        }).to_string();
        let mut pairs = complete();
        pairs.push((WORKERS_ENV, &workers));
        let config = resolve(environment(&pairs)).unwrap();
        assert_eq!(config.workers.len(), 2);
        assert_eq!(config.workers["copilot"].command, "copilot");
        assert_eq!(config.workers["claude"].command, "claude");
    }

    #[test]
    fn workers_cannot_share_the_same_working_directory() {
        let root = std::env::temp_dir();
        let workers = serde_json::json!({
            "first": {"command":"copilot", "args":["-p","{prompt}"], "working_directory":root, "branch":"agents/first"},
            "second": {"command":"claude", "args":["-p","{prompt}"], "working_directory":root, "branch":"agents/second"},
        }).to_string();
        let mut pairs = complete();
        pairs.push((WORKERS_ENV, &workers));
        assert!(matches!(
            resolve(environment(&pairs)),
            Err(ConfigError::InvalidWorkers(_))
        ));
    }

    /// An operator configuring a new node should learn about every missing
    /// variable at once. Reporting them one per run turns a single mistake
    /// into as many restarts as there are unset variables.
    #[test]
    fn every_missing_variable_is_reported_together() {
        let error =
            resolve(environment(&[])).expect_err("an incomplete environment must be refused");

        let ConfigError::Missing(names) = &error else {
            panic!("expected a missing-variable refusal, got {error:?}");
        };
        assert_eq!(
            names,
            &vec![
                NODE_STATE_DIR_ENV,
                TENANT_ID_ENV,
                REPOSITORY_ID_ENV,
                SUPERVISOR_ID_ENV
            ]
        );
        let message = error.to_string();
        assert!(
            message.contains(NODE_STATE_DIR_ENV),
            "the refusal must name the variables: {message}"
        );
    }

    /// A variable set to whitespace is a configuration mistake that looks
    /// exactly like a correct one in a shell script, so it is treated as unset
    /// rather than accepted as a blank identity.
    #[test]
    fn a_blank_variable_counts_as_missing() {
        let mut pairs = complete();
        pairs.retain(|(name, _)| *name != NODE_STATE_DIR_ENV);
        pairs.push((NODE_STATE_DIR_ENV, "   "));

        let error = resolve(environment(&pairs)).expect_err("a blank node id must be refused");

        assert_eq!(error, ConfigError::Missing(vec![NODE_STATE_DIR_ENV]));
    }

    /// Supervisor ids become local queue filenames and must not contain path separators or colons.
    #[test]
    fn supervisor_ids_cannot_escape_or_invalidate_the_state_directory() {
        for invalid in [
            "../outside",
            "supervisor:slot",
            "parent/child",
            "parent\\child",
        ] {
            let mut pairs = complete();
            pairs.retain(|(name, _)| *name != SUPERVISOR_ID_ENV);
            pairs.push((SUPERVISOR_ID_ENV, invalid));
            assert!(
                resolve(environment(&pairs)).is_err(),
                "accepted unsafe state name {invalid}"
            );
        }
    }

    #[test]
    fn an_explicit_seed_is_refused_without_a_legacy_fallback() {
        let mut pairs = complete();
        let seed = "ab".repeat(32);
        pairs.push((NODE_SIGNING_KEY_SEED_ENV, seed.as_str()));

        assert!(matches!(
            resolve(environment(&pairs)),
            Err(ConfigError::Identity(
                NodeIdentityError::LegacyConfiguration(_)
            ))
        ));
    }

    /// Even malformed legacy settings must be removed, not silently ignored.
    #[test]
    fn a_malformed_seed_is_refused_rather_than_falling_back() {
        let mut pairs = complete();
        pairs.push((NODE_SIGNING_KEY_SEED_ENV, "not-a-seed"));

        let error = resolve(environment(&pairs)).expect_err("a malformed seed must be refused");

        assert_eq!(
            error,
            ConfigError::Identity(NodeIdentityError::LegacyConfiguration(vec![
                NODE_SIGNING_KEY_SEED_ENV
            ]))
        );
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
