//! The real, network-calling `FederatedClaimAuthority` (ADR-0096 clauses 2-4,
//! 6): wired into the running engine only when this binary is built with the
//! `federation-client` feature and `CoordinationMode::Federated` was
//! resolved.
//!
//! Bridges `lodestar-core`'s synchronous trait to `ackplane-client`'s async
//! `ClaimClient` with a fresh current-thread tokio runtime per call, mirroring
//! `ackplane_core::compiled_federation_readiness`'s exact pattern: a claim
//! call is infrequent enough next to its own network round trip that a
//! runtime's startup cost is immaterial, and there is no long-lived runtime
//! state to manage between calls.

use ackplane_client::{
    authenticate, ClaimClient, ClaimLeaseOutcome, ClaimLeaseRequest, ClaimLeaseResult,
    ClaimRecoverRequest, ClaimReleaseRequest, ClaimRenewRequest, SeedSigner,
};
use lodestar_core::{
    FederatedClaimAuthority, FederatedClaimGrant, FederatedClaimOutcome,
    FederatedClaimRecoverRequest, LodestarError,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// Explicit, injected identity and endpoint configuration for a federated
/// repository (ADR-0096 clause 4). No field is ever inferred from transport
/// reachability.
pub struct FederationIdentity {
    pub endpoint: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub signing_key_id: String,
    /// The node's Ed25519 seed. Interim sourcing only (see
    /// `resolve_identity`'s doc comment); the type itself carries no opinion
    /// about where the bytes came from.
    pub signing_key_seed: [u8; 32],
}

const TENANT_ID_ENV: &str = "MINDLEAK_ACKPLANE_TENANT_ID";
const REPOSITORY_ID_ENV: &str = "MINDLEAK_ACKPLANE_REPOSITORY_ID";
const NODE_ID_ENV: &str = "MINDLEAK_ACKPLANE_NODE_ID";
const SIGNING_KEY_ID_ENV: &str = "MINDLEAK_ACKPLANE_SIGNING_KEY_ID";
/// The interim, explicit-configuration key source (a hex-encoded 32-byte
/// Ed25519 seed), the same posture already accepted here for
/// `MINDLEAK_LLM_API_KEY`. ADR-0085 decision 2's OS-credential-facility
/// storage remains future hardening -- see
/// `gaps.d/the-node-signing-key-has-no-credential-facility-yet.md`. Never
/// logged.
const NODE_SIGNING_KEY_SEED_ENV: &str = "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED";

/// Read this repository's federated claim identity from explicit environment
/// variables. `None` if any is unset, blank, or malformed -- the caller
/// decides what that means (this binary: refuse to serve rather than guess).
pub fn resolve_identity<F>(environment: F) -> Option<FederationIdentity>
where
    F: Fn(&str) -> Option<String>,
{
    Some(FederationIdentity {
        endpoint: non_empty(environment(ackplane_core::ACKPLANE_ENDPOINT_ENV))?,
        tenant_id: non_empty(environment(TENANT_ID_ENV))?,
        repository_id: non_empty(environment(REPOSITORY_ID_ENV))?,
        node_id: non_empty(environment(NODE_ID_ENV))?,
        signing_key_id: non_empty(environment(SIGNING_KEY_ID_ENV))?,
        signing_key_seed: decode_seed(&non_empty(environment(NODE_SIGNING_KEY_SEED_ENV))?)?,
    })
}

/// Every configuration variable [`resolve_identity`] reads, for a refusal
/// message that names what is missing rather than only that something is.
pub const IDENTITY_ENV_VARS: &[&str] = &[
    ackplane_core::ACKPLANE_ENDPOINT_ENV,
    TENANT_ID_ENV,
    REPOSITORY_ID_ENV,
    NODE_ID_ENV,
    SIGNING_KEY_ID_ENV,
    NODE_SIGNING_KEY_SEED_ENV,
];

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn decode_seed(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut seed = [0u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(seed)
}

pub struct AckplaneClaimAuthority {
    identity: FederationIdentity,
}

impl AckplaneClaimAuthority {
    pub fn new(identity: FederationIdentity) -> Self {
        Self { identity }
    }

    fn signer(&self) -> SeedSigner {
        SeedSigner::new(
            self.identity.signing_key_id.clone(),
            self.identity.node_id.clone(),
            &self.identity.signing_key_seed,
        )
    }

    fn runtime() -> lodestar_core::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                LodestarError::Federated(format!(
                    "could not start a runtime to reach Ackplane: {error}"
                ))
            })
    }
}

impl FederatedClaimAuthority for AckplaneClaimAuthority {
    fn delegate(
        &self,
        task_id: &str,
        owner: &str,
        branch: Option<&str>,
        lease_secs: i64,
        paths: &[String],
        symbols: &[String],
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        let identity = &self.identity;
        let authentication = authenticate(
            &self.signer(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            owner,
        );
        let request = ClaimLeaseRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            branch: branch.unwrap_or_default().to_string(),
            lease_seconds: lease_secs.max(0) as u64,
            paths: paths.to_vec(),
            symbols: symbols.to_vec(),
            authentication: Some(authentication),
        };
        let result = Self::runtime()?
            .block_on(async {
                let mut client = ClaimClient::connect(&identity.endpoint).await?;
                client.delegate_claim(request).await
            })
            .map_err(map_client_error)?;
        outcome_from_result(result)
    }

    fn renew(
        &self,
        task_id: &str,
        owner: &str,
        lease_secs: i64,
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        let identity = &self.identity;
        let authentication = authenticate(
            &self.signer(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            owner,
        );
        let request = ClaimRenewRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            lease_seconds: lease_secs.max(0) as u64,
            authentication: Some(authentication),
        };
        let result = Self::runtime()?
            .block_on(async {
                let mut client = ClaimClient::connect(&identity.endpoint).await?;
                client.renew_claim(request).await
            })
            .map_err(map_client_error)?;
        outcome_from_result(result)
    }

    fn release(&self, task_id: &str, owner: &str) -> lodestar_core::Result<bool> {
        let identity = &self.identity;
        let authentication = authenticate(
            &self.signer(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            owner,
        );
        let request = ClaimReleaseRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            authentication: Some(authentication),
        };
        let result = Self::runtime()?
            .block_on(async {
                let mut client = ClaimClient::connect(&identity.endpoint).await?;
                client.release_claim(request).await
            })
            .map_err(map_client_error)?;
        Ok(result.released)
    }

    fn recover(
        &self,
        request: &FederatedClaimRecoverRequest,
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        let identity = &self.identity;
        let authentication = authenticate(
            &self.signer(),
            &identity.tenant_id,
            &identity.repository_id,
            &request.task_id,
            &request.owner,
        );
        let wire_request = ClaimRecoverRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: request.task_id.clone(),
            expected_owner: request.expected_owner.clone(),
            owner_id: request.owner.clone(),
            reason: request.reason.clone(),
            branch: request.branch.clone().unwrap_or_default(),
            lease_seconds: request.lease_secs.max(0) as u64,
            paths: request.paths.clone(),
            symbols: request.symbols.clone(),
            authentication: Some(authentication),
        };
        let result = Self::runtime()?
            .block_on(async {
                let mut client = ClaimClient::connect(&identity.endpoint).await?;
                client.recover_claim(wire_request).await
            })
            .map_err(map_client_error)?;
        outcome_from_result(result)
    }
}

fn map_client_error(error: ackplane_client::ClientError) -> LodestarError {
    LodestarError::Federated(format!("Ackplane arbiter: {error}"))
}

fn parse_unix_seconds(rfc3339: &str) -> lodestar_core::Result<i64> {
    OffsetDateTime::parse(rfc3339, &Rfc3339)
        .map(|parsed| parsed.unix_timestamp())
        .map_err(|error| {
            LodestarError::Federated(format!(
                "Ackplane returned an unparseable timestamp {rfc3339:?}: {error}"
            ))
        })
}

fn outcome_from_result(result: ClaimLeaseResult) -> lodestar_core::Result<FederatedClaimOutcome> {
    match result.outcome() {
        ClaimLeaseOutcome::Granted => Ok(FederatedClaimOutcome::Granted(FederatedClaimGrant {
            owner: result.owner_id,
            branch: (!result.branch.is_empty()).then_some(result.branch),
            claim_started_at: parse_unix_seconds(&result.claim_started_at)?,
            lease_expires_at: parse_unix_seconds(&result.lease_expires_at)?,
            claim_lapses: result.claim_lapses as i64,
            paths: result.paths,
            symbols: result.symbols,
        })),
        ClaimLeaseOutcome::Rejected | ClaimLeaseOutcome::Unspecified => {
            let diagnostic = if result.diagnostic.is_empty() {
                format!(
                    "held by {} until {}",
                    result.owner_id, result.lease_expires_at
                )
            } else {
                result.diagnostic
            };
            Ok(FederatedClaimOutcome::Rejected { diagnostic })
        }
    }
}

#[cfg(test)]
mod tests {
    //! Opt-in, real-server, real-Postgres end-to-end proof that `task_claim`
    //! actually reaches Ackplane through this module's production
    //! `AckplaneClaimAuthority` (ADR-0096 clause 5's acceptance criterion),
    //! not a stand-in. Mirrors `ackplane-client`'s `tests/arbitration.rs`
    //! fixture (real service, real signing key, real Postgres) rather than a
    //! second protocol harness: same signing-key registration shape, same
    //! deterministic test key, same skip-unless-configured guard.
    //!
    //! Run against the compose topology with, e.g.:
    //! ```text
    //! docker compose up -d postgres
    //! ACKPLANE_TEST_DATABASE_URL=postgres://ackplane:ackplane-development-only-not-for-production@127.0.0.1:5432/ackplane cargo test -p lodestar-mcp --features federation-client --test-threads 1 federated_task_claim
    //! ```

    use std::sync::Arc;

    use ackplane_protocol::v1::claim_delegation_service_server::ClaimDelegationServiceServer;
    use ackplane_server::{
        claim_service::ClaimDelegationService,
        claim_store::ClaimStore,
        enrollment_store::EnrollmentStore,
        signing_keys::{self, SigningKeyRecord},
    };
    use ed25519_dalek::SigningKey;
    use lodestar_core::{Lodestar, TaskScope};
    use tokio::sync::oneshot;
    use tokio_postgres::NoTls;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    use super::{AckplaneClaimAuthority, FederationIdentity};

    const SIGNING_KEY_ID: &str = "lodestar-mcp-federation-test-key";
    const NODE_ID: &str = "lodestar-mcp-federation-test-node";
    const TENANT_ID: &str = "lodestar-mcp-federation-test-tenant";
    const REPOSITORY_ID: &str = "lodestar-mcp-federation-test-repository";

    fn seed() -> [u8; 32] {
        [23; 32]
    }

    fn unique_task_id() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("lodestar-mcp-federation-test-{nanos}")
    }

    async fn register_test_key(database_url: &str) {
        drop(
            EnrollmentStore::connect(database_url)
                .await
                .expect("the gated test database should accept enrollment migrations"),
        );
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls)
            .await
            .expect("the gated test database should accept a signing-key connection");
        tokio::spawn(async move {
            connection
                .await
                .expect("the signing-key fixture connection should stay healthy");
        });
        let transaction = client
            .transaction()
            .await
            .expect("the signing-key fixture should start a transaction");
        let key = SigningKey::from_bytes(&seed());
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: SIGNING_KEY_ID.to_string(),
                tenant_id: TENANT_ID.to_string(),
                repository_id: REPOSITORY_ID.to_string(),
                node_id: NODE_ID.to_string(),
                public_key: key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: SIGNING_KEY_ID.to_string(),
                activated_at: std::time::SystemTime::UNIX_EPOCH,
                expires_at: None,
            },
        )
        .await
        .expect("the fixture key should register through the production schema");
        transaction
            .commit()
            .await
            .expect("the fixture key should commit");
    }

    /// The full stack: `Lodestar::claim_task_with_partial_scope`/`renew_lease`/
    /// `release_task`, through `AckplaneClaimAuthority`, over a real gRPC
    /// connection, against a real `ClaimDelegationService` backed by real
    /// Postgres -- proof this module's production wiring, not a fake, is
    /// what `task_claim` actually reaches in federated mode.
    #[tokio::test]
    async fn task_claim_round_trips_through_a_real_ackplane_deployment() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        register_test_key(&database_url).await;
        let store = ClaimStore::connect(&database_url)
            .await
            .expect("the gated test database should accept claim migrations");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the authenticated test service should bind loopback");
        let address = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(ClaimDelegationServiceServer::new(
                    ClaimDelegationService::new(store),
                ))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("the authenticated test service should run");
        });

        let identity = FederationIdentity {
            endpoint: format!("http://{address}"),
            tenant_id: TENANT_ID.to_string(),
            repository_id: REPOSITORY_ID.to_string(),
            node_id: NODE_ID.to_string(),
            signing_key_id: SIGNING_KEY_ID.to_string(),
            signing_key_seed: seed(),
        };
        let authority = Arc::new(AckplaneClaimAuthority::new(identity));

        // `AckplaneClaimAuthority` blocks its own runtime per call (mirroring
        // `compiled_federation_readiness`), so it must run off the async test
        // executor's thread, exactly as the synchronous MCP dispatch loop
        // that calls it in production does.
        tokio::task::spawn_blocking(move || {
            let engine = Lodestar::open_in_memory()
                .unwrap()
                .with_federated_claim_authority(authority);
            let agent = "local-agent";
            engine
                .declare_session_context(
                    agent,
                    &mindleak_session::SessionContext {
                        branch: Some("feat/federated-claim-routing".to_string()),
                        ..Default::default()
                    },
                )
                .unwrap();
            let goal = engine
                .define_goal(
                    lodestar_core::GoalKind::Objective,
                    "Federate",
                    "prove the round trip",
                    None,
                )
                .unwrap();
            let task = engine
                .create_task(
                    &goal.id,
                    &format!("Route a claim through Ackplane {}", unique_task_id()),
                    "done",
                )
                .unwrap();
            let task_id = task.id;
            let scope = TaskScope {
                paths: vec!["src/lib.rs".to_string()],
                symbols: Vec::new(),
            };

            let won = engine
                .claim_task_with_partial_scope(
                    &task_id,
                    agent,
                    60,
                    Some(&scope.paths),
                    Some(&scope.symbols),
                )
                .expect("claim_task_with_partial_scope should round-trip over the wire");
            assert!(won, "a fresh task should be granted");
            let after_claim = engine.store().get_task(&task_id).unwrap().unwrap();
            assert!(
                after_claim.owner.is_some(),
                "the grant should populate owner"
            );
            assert_eq!(
                after_claim.lease_expires_at,
                after_claim.claim_started_at.map(|started| started + 60)
            );

            let owner = after_claim.owner.clone().unwrap();
            let renewed = engine
                .renew_lease(&task_id, &owner, 120)
                .expect("renew_lease should round-trip over the wire");
            assert!(renewed);

            let released = engine
                .release_task(&task_id, &owner)
                .expect("release_task should round-trip over the wire");
            assert!(released);
            let after_release = engine.store().get_task(&task_id).unwrap().unwrap();
            assert_eq!(
                after_release.owner, None,
                "release should clear the local cache"
            );
        })
        .await
        .unwrap();

        let _ = shutdown_tx.send(());
        server.await.unwrap();
    }
}
