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

use std::fmt;

pub mod ledger;

use thiserror::Error;

/// Where the ledger lives. Named, never defaulted.
const DATABASE_URL_ENV: &str = "ACKPLANE_DATABASE_URL";
/// The address to serve on. Defaults to loopback (ADR-0088 clause 6).
const LISTEN_ENV: &str = "ACKPLANE_LISTEN";
/// The durability profile this deployment reports (ADR-0086 clause 12).
const DURABILITY_ENV: &str = "ACKPLANE_DURABILITY";
/// The synchronous standbys a `quorum_durable` claim rests on.
const SYNCHRONOUS_STANDBYS_ENV: &str = "ACKPLANE_SYNCHRONOUS_STANDBYS";

/// A development deployment binds here, so a misconfigured server is reachable
/// from the machine that started it and nowhere else.
const DEFAULT_LISTEN: &str = "127.0.0.1:8443";

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
    pub listen: String,
    /// Never rendered anywhere. A PostgreSQL URL carries a password, and the
    /// places it would leak are the banner and a debug dump.
    database_url: String,
    pub durability: DurabilityProfile,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("listen", &self.listen)
            .field("database_url", &"<redacted>")
            .field("durability", &self.durability)
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

        let listen = value(LISTEN_ENV).unwrap_or_else(|| DEFAULT_LISTEN.to_string());

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

        Ok(Self {
            listen,
            database_url,
            durability,
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

        assert_eq!(config.listen, "127.0.0.1:8443");
        assert_eq!(config.durability, DurabilityProfile::SingleNode);
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
