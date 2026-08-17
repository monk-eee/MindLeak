//! The optional browser assurance interface for an Ackplane deployment.
//!
//! The standalone product remains the VSIX plus local SQLite-backed MCP
//! servers. This crate is only for the full-server deployment and begins with
//! a loopback-only developer profile while production authentication is wired.

use std::{fmt, net::SocketAddr};

use thiserror::Error;

const DATABASE_URL_ENV: &str = "ACKPLANE_DATABASE_URL";
const LISTEN_ENV: &str = "ACKPLANE_BRIDGE_LISTEN";
const DEVELOPMENT_TENANT_ENV: &str = "ACKPLANE_BRIDGE_DEVELOPMENT_TENANT";
const DEFAULT_LISTEN: &str = "127.0.0.1:3000";

/// Configuration for the loopback-only Bridge developer profile.
#[derive(Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    pub listen: SocketAddr,
    database_url: String,
    pub development_tenant: String,
}

impl fmt::Debug for BridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeConfig")
            .field("listen", &self.listen)
            .field("database_url", &"<redacted>")
            .field("development_tenant", &self.development_tenant)
            .finish()
    }
}

impl BridgeConfig {
    /// Resolve a developer profile. It deliberately refuses non-loopback
    /// binding until a production authentication verifier exists.
    pub fn resolve(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let value = |key: &str| {
            lookup(key)
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };
        let database_url = value(DATABASE_URL_ENV).ok_or(ConfigError::NoDatabase)?;
        let listen = value(LISTEN_ENV)
            .unwrap_or_else(|| DEFAULT_LISTEN.to_string())
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::InvalidListen)?;
        if !listen.ip().is_loopback() {
            return Err(ConfigError::NonLoopbackWithoutAuthentication);
        }
        let development_tenant =
            value(DEVELOPMENT_TENANT_ENV).ok_or(ConfigError::NoDevelopmentTenant)?;

        Ok(Self {
            listen,
            database_url,
            development_tenant,
        })
    }

    /// The database URL is intentionally not part of Debug output.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("ACKPLANE_DATABASE_URL must be set before the Bridge can read Ackplane projections")]
    NoDatabase,
    #[error("ACKPLANE_BRIDGE_LISTEN must be a socket address")]
    InvalidListen,
    #[error("ACKPLANE_BRIDGE_DEVELOPMENT_TENANT must be set for the loopback developer profile")]
    NoDevelopmentTenant,
    #[error(
        "the Bridge developer profile may bind only to loopback; configure a production authentication verifier before exposing it"
    )]
    NonLoopbackWithoutAuthentication,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn resolve(values: &[(&str, &str)]) -> Result<BridgeConfig, ConfigError> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<HashMap<_, _>>();
        BridgeConfig::resolve(|key| values.get(key).cloned())
    }

    #[test]
    fn a_developer_bridge_requires_a_tenant_and_stays_on_loopback() {
        let config = resolve(&[
            (DATABASE_URL_ENV, "postgresql://bridge-test"),
            (DEVELOPMENT_TENANT_ENV, "tenant-test"),
        ])
        .expect("loopback bridge configuration resolves");

        assert_eq!(config.listen, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(config.development_tenant, "tenant-test");
    }

    #[test]
    fn a_developer_bridge_refuses_a_non_loopback_listener() {
        let error = resolve(&[
            (DATABASE_URL_ENV, "postgresql://bridge-test"),
            (DEVELOPMENT_TENANT_ENV, "tenant-test"),
            (LISTEN_ENV, "0.0.0.0:3000"),
        ])
        .expect_err("a developer bridge must not be exposed remotely");

        assert_eq!(error, ConfigError::NonLoopbackWithoutAuthentication);
    }
}
