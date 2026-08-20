//! The Ackplane federation service (ADR-0082): a separately deployable arbiter
//! for one organisation boundary.
//!
//! This crate is the server side of the boundary. [`ackplane-core`] is the
//! repository side and stays that way; neither depends on the other, and
//! neither depends on a plane, because ADR-0082 clause 1 makes Ackplane a
//! separate deployable rather than a mode of `mindleak-mcp`, `lodestar-mcp`,
//! the extension, or the Bridge.
//!
//! What exists here so far is the startup contract: what a deployment must
//! declare before it may accept work, and what it is allowed to claim about its
//! own durability, plus the durable ledger schema and idempotent append
//! transaction (ADR-0086 clauses 4, 5, 6, 11) that gives it something to
//! serve from.
//!
//! [`ackplane-core`]: https://docs.rs/ackplane-core

use std::{fmt, net::SocketAddr};

pub mod claim_service;
pub mod claim_signature;
pub mod claim_store;
pub mod enrollment;
pub mod enrollment_service;
pub mod enrollment_store;
pub mod envelope_signature;
pub mod fleet;
pub mod knowledge_service;
pub mod knowledge_signature;
pub mod knowledge_store;
pub mod ledger;
mod migration_lock;
pub mod projection;
pub mod service;
pub mod signing_keys;
pub mod sync;
#[cfg(test)]
mod test_support;

use thiserror::Error;

/// Where the ledger lives. Named, never defaulted.
const DATABASE_URL_ENV: &str = "ACKPLANE_DATABASE_URL";
/// The address to serve on. Defaults to loopback (ADR-0088 clause 6).
const LISTEN_ENV: &str = "ACKPLANE_LISTEN";
/// The durability profile this deployment reports (ADR-0086 clause 12).
const DURABILITY_ENV: &str = "ACKPLANE_DURABILITY";
/// The synchronous standbys a `quorum_durable` claim rests on.
const SYNCHRONOUS_STANDBYS_ENV: &str = "ACKPLANE_SYNCHRONOUS_STANDBYS";
/// The number of event batches a node may have awaiting acknowledgement.
const MAX_IN_FLIGHT_BATCHES_ENV: &str = "ACKPLANE_MAX_IN_FLIGHT_BATCHES";
/// The largest encoded event batch a node may send.
const MAX_BATCH_BYTES_ENV: &str = "ACKPLANE_MAX_BATCH_BYTES";
/// PEM certificate for network-reachable gRPC listeners.
const TLS_CERTIFICATE_PATH_ENV: &str = "ACKPLANE_TLS_CERTIFICATE_PATH";
/// PEM private key for network-reachable gRPC listeners.
const TLS_KEY_PATH_ENV: &str = "ACKPLANE_TLS_KEY_PATH";
/// How often the projection worker polls for stale repositories (ADR-0086
/// clause 9).
const PROJECTION_INTERVAL_SECS_ENV: &str = "ACKPLANE_PROJECTION_INTERVAL_SECS";

/// A development deployment binds here, so a misconfigured server is reachable
/// from the machine that started it and nowhere else.
const DEFAULT_LISTEN: &str = "127.0.0.1:8443";
/// A node may queue at most this many unacknowledged batches by default.
pub const DEFAULT_MAX_IN_FLIGHT_BATCHES: u32 = 16;
/// A node may send at most one mebibyte in a batch by default.
pub const DEFAULT_MAX_BATCH_BYTES: u32 = 1_048_576;
/// How often, in seconds, the projection worker checks for repositories whose
/// ledger has moved past their projection checkpoint.
pub const DEFAULT_PROJECTION_INTERVAL_SECS: u32 = 5;

/// The PEM files a Tonic listener needs to authenticate its endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    pub certificate: String,
    pub key: String,
}

/// What a deployment is entitled to say about the durability of an
/// acknowledgement (ADR-0086 clause 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurabilityProfile {
    /// One PostgreSQL primary. Durable WAL commits, no replication guarantee.
    SingleNode,
    /// Acknowledged commits are synchronously replicated to the named failure
    /// domain. The names are carried because the claim is only meaningful with
    /// them: "quorum durable" on its own is a label, and clause 12 exists to
    /// stop an asynchronously replicated acknowledgement wearing it.
    QuorumDurable { synchronous_standbys: Vec<String> },
}

impl DurabilityProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SingleNode => "single_node",
            Self::QuorumDurable { .. } => "quorum_durable",
        }
    }
}

impl fmt::Display for DurabilityProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SingleNode => f.write_str("single_node"),
            Self::QuorumDurable {
                synchronous_standbys,
            } => write!(
                f,
                "quorum_durable across {}",
                synchronous_standbys.join(", ")
            ),
        }
    }
}

/// Everything a deployment must settle before it can accept work.
///
/// `Debug` is hand-written rather than derived: the derived one would print the
/// database URL, and a config struct reaches a log through `{:?}` at least as
/// easily as through the banner.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    /// Never rendered anywhere. A PostgreSQL URL carries a password, and the
    /// places it would leak are the banner and a debug dump.
    database_url: String,
    pub durability: DurabilityProfile,
    pub max_in_flight_batches: u32,
    pub max_batch_bytes: u32,
    pub projection_interval_secs: u32,
    pub tls: Option<TlsPaths>,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("listen", &self.listen)
            .field("database_url", &"<redacted>")
            .field("durability", &self.durability)
            .field("max_in_flight_batches", &self.max_in_flight_batches)
            .field("max_batch_bytes", &self.max_batch_bytes)
            .field("projection_interval_secs", &self.projection_interval_secs)
            .field("tls", &self.tls)
            .finish()
    }
}

impl ServerConfig {
    /// Resolve configuration from a lookup rather than from the process
    /// environment, so the refusals below are testable without a live process.
    pub fn resolve(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let value = |key: &str| {
            lookup(key)
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };

        // ADR-0086 clause 1: an instance holds no authoritative local state and
        // cannot accept work without the ledger. Starting without one and
        // failing later would make the outage look like a bug in whatever
        // request happened to arrive first.
        let database_url = value(DATABASE_URL_ENV).ok_or(ConfigError::NoDatabase)?;

        let listen = value(LISTEN_ENV)
            .unwrap_or_else(|| DEFAULT_LISTEN.to_string())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::InvalidListen(error.to_string()))?;

        let certificate = value(TLS_CERTIFICATE_PATH_ENV);
        let key = value(TLS_KEY_PATH_ENV);
        let tls = match (certificate, key) {
            (Some(certificate), Some(key)) => Some(TlsPaths { certificate, key }),
            (None, None) if listen.ip().is_loopback() => None,
            (None, None) => return Err(ConfigError::NonLoopbackWithoutTls),
            (certificate, _) => {
                return Err(ConfigError::IncompleteTlsMaterial {
                    missing: if certificate.is_none() {
                        TLS_CERTIFICATE_PATH_ENV
                    } else {
                        TLS_KEY_PATH_ENV
                    },
                })
            }
        };

        let standbys: Vec<String> = value(SYNCHRONOUS_STANDBYS_ENV)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let durability = match value(DURABILITY_ENV).as_deref() {
            None | Some("single_node") => DurabilityProfile::SingleNode,
            Some("quorum_durable") if standbys.is_empty() => {
                return Err(ConfigError::UnbackedQuorumClaim)
            }
            Some("quorum_durable") => DurabilityProfile::QuorumDurable {
                synchronous_standbys: standbys,
            },
            Some(other) => return Err(ConfigError::UnknownDurability(other.to_string())),
        };

        let max_in_flight_batches = positive_limit(
            value(MAX_IN_FLIGHT_BATCHES_ENV),
            MAX_IN_FLIGHT_BATCHES_ENV,
            DEFAULT_MAX_IN_FLIGHT_BATCHES,
        )?;
        let max_batch_bytes = positive_limit(
            value(MAX_BATCH_BYTES_ENV),
            MAX_BATCH_BYTES_ENV,
            DEFAULT_MAX_BATCH_BYTES,
        )?;
        let projection_interval_secs = positive_limit(
            value(PROJECTION_INTERVAL_SECS_ENV),
            PROJECTION_INTERVAL_SECS_ENV,
            DEFAULT_PROJECTION_INTERVAL_SECS,
        )?;

        Ok(Self {
            listen,
            database_url,
            durability,
            max_in_flight_batches,
            max_batch_bytes,
            projection_interval_secs,
            tls,
        })
    }

    /// The line written at startup (ADR-0088 clause 6): a deployment says which
    /// profile it is running before anyone has to guess from behaviour. It
    /// names the durability claim and the address, and deliberately never the
    /// database URL, which carries a password.
    pub fn banner(&self) -> String {
        format!(
            "ackplane-server {} listening on {}; durability {}",
            env!("CARGO_PKG_VERSION"),
            self.listen,
            self.durability
        )
    }

    /// The connection string the ledger is reached through. Never logged or
    /// rendered (see the hand-written `Debug` impl above); this exists only
    /// for the one caller that actually opens the connection.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error(
        "{DATABASE_URL_ENV} is not set, so this deployment has no ledger. Ackplane holds no \
         authoritative local state and cannot accept work without one (ADR-0086 clause 1); \
         refusing to start rather than failing on the first request that arrives"
    )]
    NoDatabase,
    #[error(
        "{DURABILITY_ENV} is `quorum_durable` but {SYNCHRONOUS_STANDBYS_ENV} names no standby. \
         A durability claim that nothing backs is exactly the asynchronously replicated \
         acknowledgement ADR-0086 clause 12 refuses to label as zero-loss; declare the \
         synchronous failure domain or report `single_node`"
    )]
    UnbackedQuorumClaim,
    #[error(
        "{DURABILITY_ENV} is `{0}`, which is not a durability profile; expected `single_node` \
         or `quorum_durable`. Refusing to guess: the wrong guess here is a deployment \
         overstating what it can promise"
    )]
    UnknownDurability(String),
    #[error("{LISTEN_ENV} is `{0}`, which is not a socket address such as `{DEFAULT_LISTEN}")]
    InvalidListen(String),
    #[error(
        "{LISTEN_ENV} is not loopback but neither {TLS_CERTIFICATE_PATH_ENV} nor {TLS_KEY_PATH_ENV} is set; \
         ADR-0083 clause 8 requires TLS outside loopback"
    )]
    NonLoopbackWithoutTls,
    #[error(
        "TLS material is incomplete: set both {TLS_CERTIFICATE_PATH_ENV} and {TLS_KEY_PATH_ENV}; \
         missing {missing}"
    )]
    IncompleteTlsMaterial { missing: &'static str },
    #[error("{variable} must be a positive unsigned integer, got `{value}`")]
    InvalidLimit {
        variable: &'static str,
        value: String,
    },
}

fn positive_limit(
    value: Option<String>,
    variable: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
    match value {
        None => Ok(default),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|limit| *limit > 0)
            .ok_or(ConfigError::InvalidLimit { variable, value }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a lookup over a fixed set of pairs, so no test touches the real
    /// process environment or the tests race each other.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn a_deployment_without_a_ledger_refuses_to_start() {
        let error = ServerConfig::resolve(env(&[])).unwrap_err();

        assert_eq!(error, ConfigError::NoDatabase);
        assert!(
            error.to_string().contains("ADR-0086 clause 1"),
            "the refusal cites what it is enforcing: {error}"
        );
    }

    /// ADR-0088 clause 6: development defaults are obviously non-production. A
    /// server that defaulted to a public interface would be one forgotten
    /// environment variable away from serving an organisation boundary to the
    /// network by accident.
    #[test]
    fn the_default_deployment_is_loopback_and_claims_only_single_node() {
        let config = ServerConfig::resolve(env(&[(
            DATABASE_URL_ENV,
            "postgres://dev@localhost/ackplane",
        )]))
        .unwrap();

        assert_eq!(config.listen, DEFAULT_LISTEN.parse().unwrap());
        assert_eq!(config.durability, DurabilityProfile::SingleNode);
        assert_eq!(config.max_in_flight_batches, DEFAULT_MAX_IN_FLIGHT_BATCHES);
        assert_eq!(config.max_batch_bytes, DEFAULT_MAX_BATCH_BYTES);
        assert_eq!(
            config.projection_interval_secs,
            DEFAULT_PROJECTION_INTERVAL_SECS
        );
        assert_eq!(config.tls, None);
    }

    /// ADR-0083 clause 8: a network-reachable Ackplane endpoint must never
    /// accidentally serve plaintext gRPC because its TLS paths were omitted.
    #[test]
    fn a_non_loopback_listener_without_tls_material_refuses_to_start() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (LISTEN_ENV, "0.0.0.0:8443"),
        ]))
        .unwrap_err();

        assert_eq!(error, ConfigError::NonLoopbackWithoutTls);
    }

    #[test]
    fn a_non_loopback_listener_resolves_both_tls_paths() {
        let config = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (LISTEN_ENV, "0.0.0.0:8443"),
            (TLS_CERTIFICATE_PATH_ENV, "cert.pem"),
            (TLS_KEY_PATH_ENV, "key.pem"),
        ]))
        .unwrap();

        assert_eq!(
            config.tls,
            Some(TlsPaths {
                certificate: "cert.pem".to_string(),
                key: "key.pem".to_string(),
            })
        );
    }

    #[test]
    fn partial_tls_material_is_refused_even_for_loopback() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (TLS_CERTIFICATE_PATH_ENV, "cert.pem"),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            ConfigError::IncompleteTlsMaterial {
                missing: TLS_KEY_PATH_ENV
            }
        );
    }

    #[test]
    fn flow_control_limits_are_resolved_from_explicit_deployment_configuration() {
        let config = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (MAX_IN_FLIGHT_BATCHES_ENV, "7"),
            (MAX_BATCH_BYTES_ENV, "8192"),
        ]))
        .unwrap();

        assert_eq!(config.max_in_flight_batches, 7);
        assert_eq!(config.max_batch_bytes, 8_192);
    }

    #[test]
    fn an_invalid_flow_control_limit_is_refused_rather_than_silently_disabled() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (MAX_IN_FLIGHT_BATCHES_ENV, "0"),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            ConfigError::InvalidLimit {
                variable: MAX_IN_FLIGHT_BATCHES_ENV,
                value: "0".to_string(),
            }
        );
    }

    #[test]
    fn the_projection_interval_is_resolved_from_explicit_deployment_configuration() {
        let config = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (PROJECTION_INTERVAL_SECS_ENV, "30"),
        ]))
        .unwrap();

        assert_eq!(config.projection_interval_secs, 30);
    }

    #[test]
    fn an_invalid_projection_interval_is_refused_rather_than_silently_disabled() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (PROJECTION_INTERVAL_SECS_ENV, "0"),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            ConfigError::InvalidLimit {
                variable: PROJECTION_INTERVAL_SECS_ENV,
                value: "0".to_string(),
            }
        );
    }

    /// ADR-0086 clause 12: a `quorum_durable` claim means an acknowledged commit
    /// is synchronously replicated to a configured failure domain. Taking the
    /// word on its own would let a single node with an environment variable
    /// report zero-loss assurance, which is the labelling that clause forbids.
    #[test]
    fn a_quorum_claim_with_no_standby_behind_it_is_refused() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (DURABILITY_ENV, "quorum_durable"),
        ]))
        .unwrap_err();

        assert_eq!(error, ConfigError::UnbackedQuorumClaim);
    }

    #[test]
    fn a_quorum_claim_carries_the_failure_domain_it_rests_on() {
        let config = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (DURABILITY_ENV, "quorum_durable"),
            (SYNCHRONOUS_STANDBYS_ENV, "standby-a, standby-b"),
        ]))
        .unwrap();

        assert_eq!(
            config.durability,
            DurabilityProfile::QuorumDurable {
                synchronous_standbys: vec!["standby-a".to_string(), "standby-b".to_string()],
            }
        );
        assert!(config.banner().contains("standby-a, standby-b"));
    }

    #[test]
    fn an_unrecognised_durability_profile_is_refused_rather_than_guessed() {
        let error = ServerConfig::resolve(env(&[
            (DATABASE_URL_ENV, "postgres://dev@localhost/ackplane"),
            (DURABILITY_ENV, "probably_fine"),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            ConfigError::UnknownDurability("probably_fine".to_string())
        );
    }

    /// A PostgreSQL URL carries a password, and the banner is the one thing
    /// this process writes before it does anything else — so it is the one
    /// place a credential would reach a log aggregator unasked.
    #[test]
    fn the_startup_banner_never_carries_the_database_credential() {
        let config = ServerConfig::resolve(env(&[(
            DATABASE_URL_ENV,
            "postgres://ackplane:hunter2@db.internal:5432/ackplane",
        )]))
        .unwrap();

        let banner = config.banner();
        assert!(!banner.contains("hunter2"), "banner leaked a password");
        assert!(!banner.contains("db.internal"), "banner leaked the host");
        assert!(banner.contains("single_node"), "banner names the profile");

        // The other door out: a derived Debug would have printed the whole URL,
        // and a config struct reaches a log through `{:?}` just as easily.
        let debugged = format!("{config:?}");
        assert!(!debugged.contains("hunter2"), "debug leaked a password");
        assert!(debugged.contains("<redacted>"), "{debugged}");
    }

    /// Whitespace-only is not a value. An empty variable set by a shell or an
    /// orchestrator template that failed to substitute must read as absent, or
    /// the server starts with a ledger URL of "".
    #[test]
    fn a_blank_variable_reads_as_absent() {
        let error = ServerConfig::resolve(env(&[(DATABASE_URL_ENV, "   ")])).unwrap_err();

        assert_eq!(error, ConfigError::NoDatabase);
    }
}
