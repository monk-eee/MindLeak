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
