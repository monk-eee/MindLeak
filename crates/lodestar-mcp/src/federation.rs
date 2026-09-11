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
    companion::{
        wire::{Claim, Operation},
        NodeClient,
    },
    node_identity, ClaimLeaseOutcome, ClaimLeaseResult, ClaimParkResult, ClaimReleaseResult,
};
use lodestar_core::{
    FederatedClaimAuthority, FederatedClaimGrant, FederatedClaimOutcome,
    FederatedClaimRecoverRequest, LodestarError,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// Resolve only the companion's public endpoint and scope, never its key.
pub fn resolve_identity<F>(environment: F) -> Option<NodeClient>
where
    F: Fn(&str) -> Option<String>,
{
    node_identity::resolve_node_client(&environment).ok()
}

pub const IDENTITY_ENV_VARS: &[&str] = node_identity::NODE_IDENTITY_ENV_VARS;

pub struct AckplaneClaimAuthority {
    node: NodeClient,
}

impl AckplaneClaimAuthority {
    pub fn new(node: NodeClient) -> Self {
        Self { node }
    }

    fn lease(&self, claim: Claim) -> lodestar_core::Result<FederatedClaimOutcome> {
        let result = Self::runtime()?
            .block_on(self.node.protobuf(Operation::Claim(claim)))
            .map_err(map_client_error)?;
        outcome_from_result(result)
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
        self.lease(Claim::Delegate {
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            branch: branch.unwrap_or_default().to_string(),
            lease_seconds: lease_secs.max(0) as u64,
            paths: paths.to_vec(),
            symbols: symbols.to_vec(),
        })
    }

    fn renew(
        &self,
        task_id: &str,
        owner: &str,
        lease_secs: i64,
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        self.lease(Claim::Renew {
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            lease_seconds: lease_secs.max(0) as u64,
        })
    }

    fn release(&self, task_id: &str, owner: &str) -> lodestar_core::Result<bool> {
        let result: ClaimReleaseResult = Self::runtime()?
            .block_on(self.node.protobuf(Operation::Claim(Claim::Release {
                task_id: task_id.to_string(),
                owner_id: owner.to_string(),
            })))
            .map_err(map_client_error)?;
        Ok(result.released)
    }

    fn recover(
        &self,
        request: &FederatedClaimRecoverRequest,
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        self.lease(Claim::Recover {
            task_id: request.task_id.clone(),
            expected_owner: request.expected_owner.clone(),
            owner_id: request.owner.clone(),
            reason: request.reason.clone(),
            branch: request.branch.clone().unwrap_or_default(),
            lease_seconds: request.lease_secs.max(0) as u64,
            paths: request.paths.clone(),
            symbols: request.symbols.clone(),
        })
    }

    fn park(&self, task_id: &str, owner: &str) -> lodestar_core::Result<bool> {
        let result: ClaimParkResult = Self::runtime()?
            .block_on(self.node.protobuf(Operation::Claim(Claim::Park {
                task_id: task_id.to_string(),
                owner_id: owner.to_string(),
            })))
            .map_err(map_client_error)?;
        Ok(result.parked)
    }

    fn answer(
        &self,
        task_id: &str,
        owner: &str,
        lease_secs: i64,
    ) -> lodestar_core::Result<FederatedClaimOutcome> {
        self.lease(Claim::Answer {
            task_id: task_id.to_string(),
            owner_id: owner.to_string(),
            lease_seconds: lease_secs.max(0) as u64,
        })
    }
}

fn map_client_error(error: impl Into<ackplane_client::ClientError>) -> LodestarError {
    LodestarError::Federated(format!("Ackplane client: {}", error.into()))
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
#[path = "../../ackplane-node/tests/support/companion.rs"]
mod companion;

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

    use super::{
        companion, resolve_identity, AckplaneClaimAuthority, NodeClient, IDENTITY_ENV_VARS,
    };

    const SIGNING_KEY_ID: &str = "lodestar-mcp-federation-test-key";
    const NODE_ID: &str = "lodestar-mcp-federation-test-node";
    const TENANT_ID: &str = "lodestar-mcp-federation-test-tenant";
    const REPOSITORY_ID: &str = "lodestar-mcp-federation-test-repository";

    fn seed() -> [u8; 32] {
        [23; 32]
    }

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

    fn seed_hex() -> &'static str {
        "1717171717171717171717171717171717171717171717171717171717171717"
            .get(0..64)
            .unwrap()
    }

    #[test]
    fn nothing_configured_resolves_to_nothing() {
        assert!(resolve_identity(env(&[])).is_none());
    }

    #[test]
    fn a_missing_companion_directory_resolves_to_nothing() {
        assert!(resolve_identity(env(&[
            (IDENTITY_ENV_VARS[1], TENANT_ID),
            (IDENTITY_ENV_VARS[2], REPOSITORY_ID),
        ]))
        .is_none());
    }

    #[test]
    fn a_full_declaration_selects_only_the_companion() {
        let directory = std::env::temp_dir();
        let identity = resolve_identity(env(&[
            (IDENTITY_ENV_VARS[0], directory.to_str().unwrap()),
            (IDENTITY_ENV_VARS[1], TENANT_ID),
            (IDENTITY_ENV_VARS[2], REPOSITORY_ID),
        ]))
        .expect("every required variable is set");
        assert_eq!(identity.state_dir, directory);
        assert_eq!(identity.tenant_id, TENANT_ID);
        assert_eq!(identity.repository_id, REPOSITORY_ID);
    }

    #[test]
    fn a_declared_seed_is_refused_instead_of_loaded_by_the_plane() {
        let directory = std::env::temp_dir();
        let identity = resolve_identity(env(&[
            (IDENTITY_ENV_VARS[0], directory.to_str().unwrap()),
            (IDENTITY_ENV_VARS[1], TENANT_ID),
            (IDENTITY_ENV_VARS[2], REPOSITORY_ID),
            (
                ackplane_client::node_identity::NODE_SIGNING_KEY_SEED_ENV,
                seed_hex(),
            ),
        ]));
        assert!(identity.is_none());
    }

    fn unique_task_id() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("lodestar-mcp-federation-test-{nanos}")
    }

    async fn register_test_key(database_url: &str) {
        let enrollment_pool = ackplane_server::db_pool::build_pool(
            database_url,
            ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
        )
        .expect("the test pool builds from a valid database url");
        drop(
            EnrollmentStore::connect(&enrollment_pool)
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
        let pool = ackplane_server::db_pool::build_pool(
            &database_url,
            ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
        )
        .expect("the test pool builds from the gated database url");
        let store = ClaimStore::connect(&pool)
            .await
            .expect("the gated test database should accept claim migrations");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the authenticated test service should bind loopback");
        let address = listener.local_addr().unwrap();
        let sync = ackplane_server::service::NodeSyncService::new(
            ackplane_server::ledger::LedgerStore::connect(&pool)
                .await
                .unwrap(),
            ackplane_protocol::v1::FlowControl {
                max_in_flight_batches: 16,
                max_batch_bytes: 1_048_576,
            },
        );
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(
                    ackplane_protocol::v1::node_sync_service_server::NodeSyncServiceServer::new(
                        sync,
                    ),
                )
                .add_service(ClaimDelegationServiceServer::new(
                    ClaimDelegationService::new(store),
                ))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("the authenticated test service should run");
        });

        let directory = tempfile::tempdir().unwrap();
        let companion = companion::TestCompanion::start(
            &format!("http://{address}"),
            ackplane_node::SigningBinding {
                tenant_id: TENANT_ID.into(),
                repository_id: REPOSITORY_ID.into(),
                node_id: NODE_ID.into(),
                key_id: SIGNING_KEY_ID.into(),
            },
            &seed(),
            directory.path(),
        )
        .await;
        let identity = NodeClient::new(
            directory.path().into(),
            TENANT_ID.into(),
            REPOSITORY_ID.into(),
        );
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

        drop(companion);
        let _ = shutdown_tx.send(());
        server.await.unwrap();
    }
}
