//! The optional browser assurance interface for an Ackplane deployment.
//!
//! The standalone product remains the VSIX plus local SQLite-backed MCP
//! servers. This crate is only for the full-server deployment and begins with
//! a loopback-only developer profile while production authentication is wired.

use std::{fmt, fs, io, net::SocketAddr, path::Path};

use sha2::{Digest, Sha256};
use thiserror::Error;

pub mod evidence;
pub mod evidence_api;
pub mod supervisor_api;

const DATABASE_URL_ENV: &str = "ACKPLANE_DATABASE_URL";
const LISTEN_ENV: &str = "ACKPLANE_BRIDGE_LISTEN";
const DEVELOPMENT_TENANT_ENV: &str = "ACKPLANE_BRIDGE_DEVELOPMENT_TENANT";
const DEFAULT_LISTEN: &str = "127.0.0.1:3000";

/// Configuration for the loopback-only Bridge developer profile.
#[derive(Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    pub listen: SocketAddr,
    database_url: String,
    /// ADR-0098 decision 3: `hex(SHA-256(salt || tenant_name))`, not the bare
    /// `ACKPLANE_BRIDGE_DEVELOPMENT_TENANT` value, so a tenant name guessed or
    /// found in a log cannot reconstruct it without the local salt. A
    /// usability hardening of the single-operator loopback path, not
    /// authentication infrastructure: it does not satisfy ADR-0084 decision
    /// 1's OIDC requirement, and ADR-0094's refusal to bind non-loopback
    /// without a production verifier is unchanged.
    pub development_tenant_token: String,
}

impl fmt::Debug for BridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeConfig")
            .field("listen", &self.listen)
            .field("database_url", &"<redacted>")
            .field("development_tenant_token", &self.development_tenant_token)
            .finish()
    }
}

impl BridgeConfig {
    /// Resolve a developer profile. It deliberately refuses non-loopback
    /// binding until a production authentication verifier exists.
    ///
    /// `salt` is the per-installation secret ADR-0098 decision 3 requires -
    /// generated once and persisted by [`load_or_generate_salt`], never
    /// derived from anything guessable.
    pub fn resolve(
        lookup: impl Fn(&str) -> Option<String>,
        salt: &[u8],
    ) -> Result<Self, ConfigError> {
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
        if salt.is_empty() {
            return Err(ConfigError::NoDevelopmentTenantSalt);
        }

        Ok(Self {
            listen,
            database_url,
            development_tenant_token: development_tenant_token(salt, &development_tenant),
        })
    }

    /// The database URL is intentionally not part of Debug output.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

/// ADR-0098 decision 3's token: `hex(SHA-256(salt || tenant_name))`.
fn development_tenant_token(salt: &[u8], tenant_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(tenant_name.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Load the per-installation salt `path` holds, generating and persisting a
/// fresh 32-byte one on first run. This is what keeps a tenant name guessed
/// or found in a log from reconstructing the same developer-tenant token on
/// another machine (ADR-0098 decision 3).
pub fn load_or_generate_salt(path: &Path) -> io::Result<Vec<u8>> {
    if let Ok(existing) = fs::read(path) {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let mut salt = vec![0_u8; 32];
    getrandom::getrandom(&mut salt).map_err(|error| {
        io::Error::other(format!("could not generate a Bridge tenant salt: {error}"))
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &salt)?;
    Ok(salt)
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
        "a per-installation salt is required to derive the developer-tenant token (ADR-0098 decision 3)"
    )]
    NoDevelopmentTenantSalt,
    #[error(
        "the Bridge developer profile may bind only to loopback; configure a production authentication verifier before exposing it"
    )]
    NonLoopbackWithoutAuthentication,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const TEST_SALT: &[u8] = b"a-fixed-test-salt-not-a-real-secret";

    fn resolve(values: &[(&str, &str)], salt: &[u8]) -> Result<BridgeConfig, ConfigError> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<HashMap<_, _>>();
        BridgeConfig::resolve(|key| values.get(key).cloned(), salt)
    }

    #[test]
    fn a_developer_bridge_requires_a_tenant_and_stays_on_loopback() {
        let config = resolve(
            &[
                (DATABASE_URL_ENV, "postgresql://bridge-test"),
                (DEVELOPMENT_TENANT_ENV, "tenant-test"),
            ],
            TEST_SALT,
        )
        .expect("loopback bridge configuration resolves");

        assert_eq!(config.listen, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(
            config.development_tenant_token,
            development_tenant_token(TEST_SALT, "tenant-test")
        );
    }

    #[test]
    fn a_developer_bridge_refuses_a_non_loopback_listener() {
        let error = resolve(
            &[
                (DATABASE_URL_ENV, "postgresql://bridge-test"),
                (DEVELOPMENT_TENANT_ENV, "tenant-test"),
                (LISTEN_ENV, "0.0.0.0:3000"),
            ],
            TEST_SALT,
        )
        .expect_err("a developer bridge must not be exposed remotely");

        assert_eq!(error, ConfigError::NonLoopbackWithoutAuthentication);
    }

    #[test]
    fn a_developer_bridge_refuses_an_empty_salt() {
        let error = resolve(
            &[
                (DATABASE_URL_ENV, "postgresql://bridge-test"),
                (DEVELOPMENT_TENANT_ENV, "tenant-test"),
            ],
            &[],
        )
        .expect_err("an empty salt must not silently resolve");

        assert_eq!(error, ConfigError::NoDevelopmentTenantSalt);
    }

    #[test]
    fn the_developer_tenant_token_is_stable_across_resolves_once_the_salt_exists() {
        let values = [
            (DATABASE_URL_ENV, "postgresql://bridge-test"),
            (DEVELOPMENT_TENANT_ENV, "tenant-test"),
        ];
        let first = resolve(&values, TEST_SALT).expect("resolves");
        let second = resolve(&values, TEST_SALT).expect("resolves");

        assert_eq!(
            first.development_tenant_token,
            second.development_tenant_token
        );
    }

    #[test]
    fn the_developer_tenant_token_differs_from_the_bare_tenant_name() {
        let config = resolve(
            &[
                (DATABASE_URL_ENV, "postgresql://bridge-test"),
                (DEVELOPMENT_TENANT_ENV, "tenant-test"),
            ],
            TEST_SALT,
        )
        .expect("resolves");

        assert_ne!(config.development_tenant_token, "tenant-test");
        assert_eq!(config.development_tenant_token.len(), 64);
    }

    #[test]
    fn a_guessed_bare_tenant_name_alone_does_not_reproduce_the_token_without_the_real_salt() {
        let values = [
            (DATABASE_URL_ENV, "postgresql://bridge-test"),
            (DEVELOPMENT_TENANT_ENV, "tenant-test"),
        ];
        let real = resolve(&values, TEST_SALT).expect("resolves");

        // An attacker who only saw the bare tenant name logged or guessed
        // somewhere has no way to know the real installation's salt.
        let guessed_salt = b"a-different-salt-the-attacker-invented";
        let guessed = resolve(&values, guessed_salt).expect("resolves");

        assert_ne!(
            real.development_tenant_token,
            guessed.development_tenant_token
        );
    }

    #[test]
    fn load_or_generate_salt_persists_a_fresh_salt_on_first_run() {
        let path = std::env::temp_dir().join(format!(
            "ackplane-bridge-salt-fresh-{}.bin",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        let salt = load_or_generate_salt(&path).expect("generates a salt");

        assert_eq!(salt.len(), 32);
        assert_eq!(fs::read(&path).expect("salt file exists"), salt);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn load_or_generate_salt_reuses_an_existing_salt_on_later_runs() {
        let path = std::env::temp_dir().join(format!(
            "ackplane-bridge-salt-reuse-{}.bin",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        let first = load_or_generate_salt(&path).expect("generates a salt");
        let second = load_or_generate_salt(&path).expect("reuses the salt");

        assert_eq!(first, second);

        fs::remove_file(&path).ok();
    }
}
