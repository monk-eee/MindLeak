//! gRPC transport for Ackplane's telemetry domain (ADR-0105 decision 6).
//!
//! Authenticated the same way `KnowledgeService` is: every RPC verifies a
//! `TelemetryAuthentication` against the enrolled node's resolved signing
//! key before it reaches the store, in its own domain (`telemetry_auth`/
//! `telemetry_signature`, its own nonce table) rather than reusing another
//! domain's operation-shaped fields.

use std::sync::Arc;
use std::time::SystemTime;

use ackplane_protocol::telemetry_auth::TelemetryOperation;
use ackplane_protocol::v1;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::telemetry_signature::{self, TelemetryAuthRefusal};
use crate::telemetry_store::{
    ReadTelemetryRequest, RecordTelemetryRequest, TelemetryStore, TelemetryStoreError,
};

/// `bucket_seconds = 0` means "use the service default" -- one-hour buckets,
/// a reasonable default granularity for a health dashboard's sparklines.
const DEFAULT_BUCKET_SECONDS: u32 = 3600;
/// `max_points = 0` means "use the service default".
const DEFAULT_MAX_POINTS: u32 = 60;
/// However many points a caller requests, never more than this many per
/// series -- an unbounded request must not turn one read into an unbounded
/// scan or an unbounded response payload.
const MAX_MAX_POINTS: u32 = 500;

pub struct TelemetryGrpcService {
    store: Arc<Mutex<TelemetryStore>>,
}

impl TelemetryGrpcService {
    pub fn new(store: TelemetryStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Verify a telemetry request's authentication before it reaches the
    /// store (mirrors `KnowledgeGrpcService::authenticate`). Returns the
    /// authenticated node id, which is telemetry's own provenance -- never a
    /// caller-supplied field.
    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &TelemetryOperation<'_>,
        authentication: Option<&v1::TelemetryAuthentication>,
    ) -> Result<String, Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                TelemetryAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let binding = crate::signing_keys::EnvelopeBinding {
            signing_key_id: &authentication.signing_key_id,
            tenant_id,
            repository_id,
            producer_id: &authentication.node_id,
            accepted_at: SystemTime::now(),
        };
        let resolution = self
            .store
            .lock()
            .await
            .resolve_signing_key(&binding)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

        telemetry_signature::verify(
            tenant_id,
            repository_id,
            operation,
            Some(authentication),
            &resolution,
            SystemTime::now(),
        )
        .map_err(|refusal| {
            if refusal.is_authenticated_but_not_authorized() {
                Status::permission_denied(refusal.diagnostic())
            } else {
                Status::unauthenticated(refusal.diagnostic())
            }
        })?;

        // Only after a genuine signature is confirmed: a forged request must
        // never be able to burn a legitimate nonce out from under its owner.
        let fresh = self
            .store
            .lock()
            .await
            .consume_telemetry_nonce(
                &authentication.signing_key_id,
                &authentication.nonce,
                SystemTime::now(),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !fresh {
            return Err(Status::unauthenticated(
                TelemetryAuthRefusal::Replayed.diagnostic(),
            ));
        }
        Ok(authentication.node_id.clone())
    }
}

fn store_error(error: TelemetryStoreError) -> Status {
    match error {
        TelemetryStoreError::InvalidName => Status::invalid_argument("name must not be empty"),
        TelemetryStoreError::TooManyMeasurements => {
            Status::invalid_argument("an event may carry at most a bounded number of measurements")
        }
        TelemetryStoreError::InvalidMeasurementName => {
            Status::invalid_argument("measurement name must not be empty")
        }
        TelemetryStoreError::NonFiniteMeasurement => {
            Status::invalid_argument("measurement value must be finite")
        }
        TelemetryStoreError::InvalidOccurredAt => {
            Status::invalid_argument("occurred_at is not a valid RFC3339 timestamp")
        }
        TelemetryStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn parse_occurred_at(value: &str) -> Result<SystemTime, Status> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(SystemTime::from)
        .map_err(|_| store_error(TelemetryStoreError::InvalidOccurredAt))
}

fn rfc3339(timestamp: SystemTime) -> Result<String, Status> {
    OffsetDateTime::from(timestamp)
        .format(&Rfc3339)
        .map_err(|error| {
            Status::internal(format!("could not format a telemetry timestamp: {error}"))
        })
}

fn bounded_bucket_seconds(requested: u32) -> u32 {
    if requested == 0 {
        DEFAULT_BUCKET_SECONDS
    } else {
        requested
    }
}

fn bounded_max_points(requested: u32) -> u32 {
    if requested == 0 {
        DEFAULT_MAX_POINTS
    } else {
        requested.min(MAX_MAX_POINTS)
    }
}

#[tonic::async_trait]
impl v1::telemetry_service_server::TelemetryService for TelemetryGrpcService {
    async fn record_telemetry(
        &self,
        request: Request<v1::RecordTelemetryRequest>,
    ) -> Result<Response<v1::TelemetryRecord>, Status> {
        let request = request.into_inner();
        let operation = TelemetryOperation::Record {
            agent_session_id: (!request.agent_session_id.is_empty())
                .then_some(request.agent_session_id.as_str()),
            kind: request.kind,
            name: &request.name,
            outcome: request.outcome,
            duration_ms: request.duration_ms,
            occurred_at: &request.occurred_at,
            measurements: &request.measurements,
        };
        let node_id = self
            .authenticate(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                request.authentication.as_ref(),
            )
            .await?;
        let occurred_at = parse_occurred_at(&request.occurred_at)?;
        let measurements = request
            .measurements
            .iter()
            .map(|measurement| (measurement.name.clone(), measurement.value))
            .collect();

        let recorded = self
            .store
            .lock()
            .await
            .record(RecordTelemetryRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                node_id,
                agent_session_id: (!request.agent_session_id.is_empty())
                    .then_some(request.agent_session_id),
                kind: request.kind as i16,
                name: request.name,
                outcome: request.outcome as i16,
                duration_ms: request.duration_ms as i64,
                occurred_at,
                measurements,
            })
            .await
            .map_err(store_error)?;

        Ok(Response::new(v1::TelemetryRecord {
            telemetry_id: recorded.telemetry_id,
            tenant_id: recorded.tenant_id,
            repository_id: recorded.repository_id,
            node_id: recorded.node_id,
            agent_session_id: recorded.agent_session_id.unwrap_or_default(),
            kind: recorded.kind as i32,
            name: recorded.name,
            outcome: recorded.outcome as i32,
            duration_ms: recorded.duration_ms as u64,
            occurred_at: rfc3339(recorded.occurred_at)?,
            recorded_at: rfc3339(recorded.recorded_at)?,
        }))
    }

    async fn read_telemetry(
        &self,
        request: Request<v1::TelemetryReadRequest>,
    ) -> Result<Response<v1::TelemetrySnapshot>, Status> {
        let request = request.into_inner();
        let bucket_seconds = bounded_bucket_seconds(request.bucket_seconds);
        let max_points = bounded_max_points(request.max_points);
        let operation = TelemetryOperation::Read {
            kind: request.kind,
            name: (!request.name.is_empty()).then_some(request.name.as_str()),
            bucket_seconds,
            max_points,
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;

        let snapshot = self
            .store
            .lock()
            .await
            .read(ReadTelemetryRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                kind: request.kind as i16,
                name: (!request.name.is_empty()).then_some(request.name),
                bucket_seconds: bucket_seconds as i64,
                max_points: max_points as i64,
            })
            .await
            .map_err(store_error)?;

        Ok(Response::new(v1::TelemetrySnapshot {
            metrics: snapshot
                .metrics
                .into_iter()
                .map(|metric| {
                    Ok(v1::TelemetryMetric {
                        kind: metric.kind as i32,
                        name: metric.name,
                        calls: metric.calls as u64,
                        errors: metric.errors as u64,
                        currently_failing: metric.currently_failing,
                        last_success_at: metric
                            .last_success_at
                            .map(rfc3339)
                            .transpose()?
                            .unwrap_or_default(),
                        last_error_at: metric
                            .last_error_at
                            .map(rfc3339)
                            .transpose()?
                            .unwrap_or_default(),
                        average_duration_ms: metric.average_duration_ms,
                    })
                })
                .collect::<Result<Vec<_>, Status>>()?,
            series: snapshot
                .series
                .into_iter()
                .map(|series| {
                    Ok(v1::TelemetrySeries {
                        kind: series.kind as i32,
                        name: series.name,
                        points: series
                            .points
                            .into_iter()
                            .map(|point| {
                                Ok(v1::TelemetryPoint {
                                    bucket_start_at: rfc3339(point.bucket_start_at)?,
                                    calls: point.calls as u64,
                                    errors: point.errors as u64,
                                    average_duration_ms: point.average_duration_ms,
                                })
                            })
                            .collect::<Result<Vec<_>, Status>>()?,
                    })
                })
                .collect::<Result<Vec<_>, Status>>()?,
            observed_at: rfc3339(snapshot.observed_at)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use ed25519_dalek::{Signer, SigningKey};

    use ackplane_protocol::v1::telemetry_service_server::TelemetryService;

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};

    /// Deterministic key material across every test -- matching
    /// `knowledge_service.rs`'s own fixture: a fixed key is fine because each
    /// test registers it under its own freshly generated identity.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[57; 32])
    }

    /// One test's fully-isolated tenant/repository/node/key identity, so
    /// tests in the same binary never share a row and never depend on
    /// registration order.
    struct TestIdentity {
        signing_key_id: String,
        node_id: String,
        tenant_id: String,
        repository_id: String,
    }

    impl TestIdentity {
        fn fresh(label: &str) -> Self {
            let suffix = crate::test_support::uuid_ish();
            Self {
                signing_key_id: format!("telemetry-service-{label}-key-{suffix}"),
                node_id: format!("telemetry-service-{label}-node-{suffix}"),
                tenant_id: format!("telemetry-service-{label}-tenant-{suffix}"),
                repository_id: format!("telemetry-service-{label}-repository-{suffix}"),
            }
        }
    }

    async fn register_test_key(database_url: &str, identity: &TestIdentity) {
        let (mut client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
            .await
            .expect("the gated test database should accept a signing-key connection");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let transaction = client
            .transaction()
            .await
            .expect("a transaction should open for key registration");
        let key = signing_key();
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: identity.signing_key_id.clone(),
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                node_id: identity.node_id.clone(),
                public_key: key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: identity.signing_key_id.clone(),
                activated_at: SystemTime::UNIX_EPOCH,
                expires_at: None,
            },
        )
        .await
        .expect("registering the test key should succeed");
        transaction
            .commit()
            .await
            .expect("the registration transaction should commit");
    }

    /// A validly-signed `RecordTelemetryRequest` -- kind `TOOL` (1), outcome
    /// `OK` (1), no measurements, `occurred_at` pinned to "now". `nonce_byte`
    /// distinguishes requests so a test can deliberately repeat or vary it.
    fn authenticated_record_request(
        identity: &TestIdentity,
        name: &str,
        nonce_byte: u8,
    ) -> v1::RecordTelemetryRequest {
        let key = signing_key();
        let occurred_at = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
        let mut authentication = v1::TelemetryAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: 1,
            name,
            outcome: 1,
            duration_ms: 10,
            occurred_at: &occurred_at,
            measurements: &[],
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::RecordTelemetryRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            agent_session_id: String::new(),
            kind: 1,
            name: name.to_owned(),
            outcome: 1,
            duration_ms: 10,
            occurred_at,
            measurements: Vec::new(),
            authentication: Some(authentication),
        }
    }

    /// A validly-signed `TelemetryReadRequest`. `kind: 0`/`name: ""` mean
    /// "every kind"/"every name", matching the RPC's own documented default.
    /// The signed operation carries the *bounded* (post-default/clamp)
    /// bucket_seconds/max_points, matching what `read_telemetry` itself
    /// signs against -- signing the raw wire values would verify against a
    /// different operation than the one the server actually authenticates.
    fn authenticated_read_request(
        identity: &TestIdentity,
        bucket_seconds: u32,
        max_points: u32,
        nonce_byte: u8,
    ) -> v1::TelemetryReadRequest {
        let key = signing_key();
        let mut authentication = v1::TelemetryAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = TelemetryOperation::Read {
            kind: 0,
            name: None,
            bucket_seconds: bounded_bucket_seconds(bucket_seconds),
            max_points: bounded_max_points(max_points),
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::TelemetryReadRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            kind: 0,
            name: String::new(),
            bucket_seconds,
            max_points,
            authentication: Some(authentication),
        }
    }

    #[test]
    fn bounded_bucket_seconds_defaults_when_zero_and_passes_through_otherwise() {
        assert_eq!(bounded_bucket_seconds(0), DEFAULT_BUCKET_SECONDS);
        assert_eq!(bounded_bucket_seconds(120), 120);
    }

    #[test]
    fn bounded_max_points_defaults_when_zero_and_clamps_to_the_maximum() {
        assert_eq!(bounded_max_points(0), DEFAULT_MAX_POINTS);
        assert_eq!(bounded_max_points(10), 10);
        assert_eq!(bounded_max_points(MAX_MAX_POINTS + 500), MAX_MAX_POINTS);
    }

    /// The recorded event is attributed to the node the signature actually
    /// verified as, never a caller-supplied field -- this RPC has no request
    /// field naming an actor at all, so attribution can only come from
    /// authentication.
    #[tokio::test]
    async fn a_fresh_authenticated_record_is_attributed_to_the_authenticated_node() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("record");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let wire = authenticated_record_request(&identity, "cargo_test", 1);

        let recorded = service
            .record_telemetry(Request::new(wire))
            .await
            .expect("a freshly signed request must be authenticated and recorded")
            .into_inner();

        assert_eq!(recorded.tenant_id, identity.tenant_id);
        assert_eq!(recorded.repository_id, identity.repository_id);
        assert_eq!(recorded.node_id, identity.node_id);
        assert_eq!(recorded.name, "cargo_test");
        assert_eq!(recorded.kind, 1);
        assert_eq!(recorded.outcome, 1);
        assert!(!recorded.telemetry_id.is_empty());
        assert!(time::OffsetDateTime::parse(&recorded.recorded_at, &Rfc3339).is_ok());
    }

    /// Proves `authenticate` wires nonce consumption into the RPC path: the
    /// identical wire request granted the first time is refused the second
    /// time on the same (signing_key_id, nonce) pair.
    #[tokio::test]
    async fn an_identical_request_is_granted_once_then_refused_as_replayed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("replay");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let wire = authenticated_record_request(&identity, "replay_tested", 2);

        service
            .record_telemetry(Request::new(wire.clone()))
            .await
            .expect("the first, fresh request must be authenticated and recorded");

        let replayed = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("the identical (signing_key_id, nonce) pair must be refused");
        assert_eq!(replayed.code(), tonic::Code::Unauthenticated);
        assert!(
            replayed.message().contains("already been used"),
            "unexpected diagnostic: {}",
            replayed.message()
        );
    }

    /// A `signed_at` far outside the accepted clock-skew window is refused
    /// before the request ever reaches the store.
    #[tokio::test]
    async fn a_stale_signed_at_is_refused_before_the_store_runs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("stale");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "stale_tested", 3);
        // Re-sign over a `signed_at` far outside the skew window -- the
        // signature must cover the stale timestamp, or this would only prove
        // the diagnostic string exists, not that verification used it.
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        authentication.signed_at = "2020-01-01T00:00:00Z".to_owned();
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &wire.occurred_at,
            measurements: &wire.measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("a signed_at far outside the skew window must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
        assert!(
            refused.message().contains("clock-skew"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// An unsigned request (no `TelemetryAuthentication` at all) is refused
    /// before it ever reaches the store.
    #[tokio::test]
    async fn a_request_with_no_authentication_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("unsigned");
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "unsigned_tested", 4);
        wire.authentication = None;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("a request with no authentication must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
    }

    /// A real recorded event is visible through `read_telemetry`'s snapshot,
    /// exercising the RPC's own metric/series conversion (not just the store
    /// layer, which has its own dedicated tests).
    #[tokio::test]
    async fn read_telemetry_returns_a_populated_snapshot_after_a_real_record() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("read");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        service
            .record_telemetry(Request::new(authenticated_record_request(
                &identity,
                "read_after_record",
                5,
            )))
            .await
            .expect("a valid request should record telemetry");

        let snapshot = service
            .read_telemetry(Request::new(authenticated_read_request(&identity, 0, 0, 6)))
            .await
            .expect("a valid request should return a snapshot")
            .into_inner();

        assert_eq!(snapshot.metrics.len(), 1);
        let metric = &snapshot.metrics[0];
        assert_eq!(metric.name, "read_after_record");
        assert_eq!(metric.kind, 1);
        assert_eq!(metric.calls, 1);
        assert_eq!(metric.errors, 0);
        assert!(!metric.currently_failing);
        assert!(time::OffsetDateTime::parse(&snapshot.observed_at, &Rfc3339).is_ok());
    }

    /// A genuinely valid signature over a claimed identity the key was never
    /// enrolled for is authenticated but not authorized -- distinct from
    /// every other refusal, which means no real key signed the request at
    /// all. Re-signs over the mismatched `repository_id` deliberately: the
    /// signature must cover the claimed binding, or this would only prove
    /// the diagnostic string exists, not that resolution used it.
    #[tokio::test]
    async fn a_key_enrolled_for_one_repository_is_refused_for_a_different_one() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("binding-mismatch");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "binding_mismatch_tested", 7);
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        let claimed_repository_id = format!("{}-not-the-enrolled-one", identity.repository_id);
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &wire.occurred_at,
            measurements: &wire.measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &identity.tenant_id,
            &claimed_repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);
        wire.repository_id = claimed_repository_id;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("a key enrolled for a different repository must be refused");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        assert!(
            refused
                .message()
                .contains("different tenant, repository or node"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// `occurred_at` is parsed before the store ever runs, reusing the
    /// store's own `InvalidOccurredAt` diagnostic for a value that fails
    /// there too.
    #[tokio::test]
    async fn an_unparseable_occurred_at_is_refused_before_the_store_runs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("bad-occurred-at");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "bad_occurred_at_tested", 8);
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        let bad_occurred_at = "not-a-timestamp".to_owned();
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &bad_occurred_at,
            measurements: &wire.measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);
        wire.occurred_at = bad_occurred_at;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("an unparseable occurred_at must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert!(
            refused.message().contains("valid RFC3339 timestamp"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// An empty event name is refused by the store's own validation, mapped
    /// through this service's `store_error`.
    #[tokio::test]
    async fn an_empty_event_name_is_refused_by_the_store() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("empty-name");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let wire = authenticated_record_request(&identity, "", 9);

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("an empty event name must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert!(
            refused.message().contains("name must not be empty"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// More than the bounded number of measurements is refused by the
    /// store's own validation.
    #[tokio::test]
    async fn too_many_measurements_is_refused_by_the_store() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("too-many-measurements");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "too_many_measurements_tested", 10);
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        let measurements: Vec<v1::TelemetryMeasurement> = (0..21)
            .map(|i| v1::TelemetryMeasurement {
                name: format!("m{i}"),
                value: 1.0,
            })
            .collect();
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &wire.occurred_at,
            measurements: &measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);
        wire.measurements = measurements;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("more than the bounded number of measurements must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert!(
            refused.message().contains("at most"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// An empty measurement name is refused by the store's own validation.
    #[tokio::test]
    async fn an_empty_measurement_name_is_refused_by_the_store() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("empty-measurement-name");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "empty_measurement_name_tested", 11);
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        let measurements = vec![v1::TelemetryMeasurement {
            name: String::new(),
            value: 1.0,
        }];
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &wire.occurred_at,
            measurements: &measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);
        wire.measurements = measurements;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("an empty measurement name must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert!(
            refused
                .message()
                .contains("measurement name must not be empty"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// A non-finite measurement value (NaN, in this case) is refused by the
    /// store's own validation.
    #[tokio::test]
    async fn a_non_finite_measurement_value_is_refused_by_the_store() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("non-finite-measurement");
        register_test_key(&database_url, &identity).await;
        let store = TelemetryStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a telemetry-store connection");
        let service = TelemetryGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "non_finite_measurement_tested", 12);
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        let measurements = vec![v1::TelemetryMeasurement {
            name: "latency_ms".to_owned(),
            value: f64::NAN,
        }];
        let operation = TelemetryOperation::Record {
            agent_session_id: None,
            kind: wire.kind,
            name: &wire.name,
            outcome: wire.outcome,
            duration_ms: wire.duration_ms,
            occurred_at: &wire.occurred_at,
            measurements: &measurements,
        };
        let bytes = telemetry_signature::telemetry_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);
        wire.measurements = measurements;

        let refused = service
            .record_telemetry(Request::new(wire))
            .await
            .expect_err("a non-finite measurement value must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert!(
            refused.message().contains("must be finite"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }
}
