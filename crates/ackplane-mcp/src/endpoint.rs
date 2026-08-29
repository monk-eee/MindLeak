//! Which Ackplane endpoint this front door is allowed to reach.
//!
//! ADR-0136 clause 4 holds `ackplane-mcp` to a local-loopback pilot until the
//! authenticated-principal decision lands. Neither principal type that exists
//! today fits an arbitrary MCP client, so reaching a remote arbiter would mean
//! sending an enrolled node's possession proof somewhere that decision has not
//! been made about yet. The guard is deliberately about the *destination* this
//! process dials, not about a bind address: this is a client, and the question
//! is where credentials travel to.

use std::net::IpAddr;

use thiserror::Error;
use url::Url;

/// Declares the arbiter to reach; defaults to [`DEFAULT_ENDPOINT`].
pub const ENDPOINT_ENV: &str = "ACKPLANE_MCP_ENDPOINT";

/// The Compose topology's published loopback port (ADR-0088).
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8443";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EndpointError {
    #[error("{ENDPOINT_ENV}={endpoint} is not a URL this front door can parse: {reason}")]
    Unparsable { endpoint: String, reason: String },
    #[error("{ENDPOINT_ENV}={endpoint} names no host to connect to")]
    MissingHost { endpoint: String },
    #[error(
        "{ENDPOINT_ENV}={endpoint} resolves to the non-loopback host {host}. ADR-0136 clause 4 \
         holds ackplane-mcp to a local-loopback pilot until the authenticated-principal decision \
         lands, because reaching a remote arbiter would send an enrolled node's possession proof \
         off this machine under an authentication model that has not been decided. Point this at \
         a loopback address, or wait for that decision rather than working around this refusal."
    )]
    NotLoopback { endpoint: String, host: String },
}

/// Resolve the endpoint to dial, refusing anything this pilot must not reach.
pub fn resolve_endpoint<F>(environment: &F) -> Result<String, EndpointError>
where
    F: Fn(&str) -> Option<String>,
{
    let endpoint = environment(ENDPOINT_ENV)
        .map(|declared| declared.trim().to_string())
        .filter(|declared| !declared.is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());

    let parsed = Url::parse(&endpoint).map_err(|error| EndpointError::Unparsable {
        endpoint: endpoint.clone(),
        reason: error.to_string(),
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| EndpointError::MissingHost {
            endpoint: endpoint.clone(),
        })?
        .to_string();

    if !is_loopback(&host) {
        return Err(EndpointError::NotLoopback { endpoint, host });
    }
    Ok(endpoint)
}

/// `localhost` is accepted by name because that is how the supported topology
/// and every human writes it; anything else must parse as a loopback IP. A name
/// this process cannot resolve itself is refused rather than trusted.
fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
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

    #[test]
    fn an_undeclared_endpoint_falls_back_to_the_published_loopback_port() {
        assert_eq!(
            resolve_endpoint(&env(&[])),
            Ok(DEFAULT_ENDPOINT.to_string())
        );
    }

    /// A blank value is a declaration someone forgot to fill in, not a request
    /// for an empty endpoint.
    #[test]
    fn a_blank_declaration_is_ignored_rather_than_dialled() {
        assert_eq!(
            resolve_endpoint(&env(&[(ENDPOINT_ENV, "   ")])),
            Ok(DEFAULT_ENDPOINT.to_string())
        );
    }

    #[test]
    fn every_loopback_spelling_is_accepted() {
        for endpoint in [
            "http://127.0.0.1:8443",
            "http://127.0.0.2:8443",
            "https://localhost:8443",
            "http://LOCALHOST:8443",
            "http://[::1]:8443",
        ] {
            assert_eq!(
                resolve_endpoint(&env(&[(ENDPOINT_ENV, endpoint)])),
                Ok(endpoint.to_string()),
                "{endpoint} is loopback and must be reachable"
            );
        }
    }

    /// The whole point of clause 4: a remote arbiter would receive an enrolled
    /// node's possession proof under an authentication model nobody has decided
    /// yet, so it is refused by name rather than dialled.
    #[test]
    fn a_remote_endpoint_is_refused_and_names_the_host_it_refused() {
        let error = resolve_endpoint(&env(&[(ENDPOINT_ENV, "https://ackplane.example.com:8443")]))
            .expect_err("a remote arbiter must be refused in the pilot");
        assert_eq!(
            error,
            EndpointError::NotLoopback {
                endpoint: "https://ackplane.example.com:8443".to_string(),
                host: "ackplane.example.com".to_string(),
            }
        );
        assert!(
            error.to_string().contains("ADR-0136 clause 4"),
            "the refusal must say which decision it is waiting on, got: {error}"
        );
    }

    /// `0.0.0.0` is the shape most likely to be reached for by someone trying
    /// to make this work in a container, and it is not loopback.
    #[test]
    fn an_unspecified_address_is_not_treated_as_loopback() {
        assert!(matches!(
            resolve_endpoint(&env(&[(ENDPOINT_ENV, "http://0.0.0.0:8443")])),
            Err(EndpointError::NotLoopback { .. })
        ));
    }

    #[test]
    fn an_unparsable_endpoint_is_reported_rather_than_guessed() {
        assert!(matches!(
            resolve_endpoint(&env(&[(ENDPOINT_ENV, "not a url")])),
            Err(EndpointError::Unparsable { .. })
        ));
    }
}
