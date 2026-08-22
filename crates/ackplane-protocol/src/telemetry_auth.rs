//! Canonical signing bytes for the telemetry service's typed observations.
//!
//! Telemetry is a separate industrial domain: an enrolled node must sign the
//! exact observation or query it sends, but its signatures must never verify
//! as claims or knowledge operations.

use crate::signing_bytes::push_field;
use crate::v1;

/// Domain separation for telemetry request signatures.
pub const TELEMETRY_DOMAIN: &[u8] = b"mindleak.ackplane.v1.telemetry\0";

/// The exact `TelemetryService` operation and fields that a node authorizes.
#[derive(Debug, Clone, Copy)]
pub enum TelemetryOperation<'a> {
    Record {
        agent_session_id: Option<&'a str>,
        kind: i32,
        name: &'a str,
        outcome: i32,
        duration_ms: u64,
        occurred_at: &'a str,
        measurements: &'a [v1::TelemetryMeasurement],
    },
    Read {
        kind: i32,
        name: Option<&'a str>,
        bucket_seconds: u32,
        max_points: u32,
    },
}

impl TelemetryOperation<'_> {
    fn tag(&self) -> &'static str {
        match self {
            Self::Record { .. } => "record",
            Self::Read { .. } => "read",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        match self {
            Self::Record {
                agent_session_id,
                kind,
                name,
                outcome,
                duration_ms,
                occurred_at,
                measurements,
            } => {
                push_field(bytes, agent_session_id.unwrap_or("").as_bytes());
                push_field(bytes, &kind.to_be_bytes());
                push_field(bytes, name.as_bytes());
                push_field(bytes, &outcome.to_be_bytes());
                push_field(bytes, &duration_ms.to_be_bytes());
                push_field(bytes, occurred_at.as_bytes());
                push_field(bytes, &(measurements.len() as u64).to_be_bytes());
                for measurement in *measurements {
                    push_field(bytes, measurement.name.as_bytes());
                    push_field(bytes, &measurement.value.to_be_bytes());
                }
            }
            Self::Read {
                kind,
                name,
                bucket_seconds,
                max_points,
            } => {
                push_field(bytes, &kind.to_be_bytes());
                push_field(bytes, name.unwrap_or("").as_bytes());
                push_field(bytes, &bucket_seconds.to_be_bytes());
                push_field(bytes, &max_points.to_be_bytes());
            }
        }
    }
}

/// Binds a telemetry authentication envelope to one tenant, repository, and
/// exact typed operation. Every field is length-delimited so boundaries remain
/// unambiguous even when identifiers or metric names contain punctuation.
pub fn telemetry_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &TelemetryOperation,
    authentication: &v1::TelemetryAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 6] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(TELEMETRY_DOMAIN.len() + 96);
    bytes.extend_from_slice(TELEMETRY_DOMAIN);
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication(nonce: u8) -> v1::TelemetryAuthentication {
        v1::TelemetryAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            nonce: vec![nonce; 16],
            signature: Vec::new(),
        }
    }

    fn measurements() -> Vec<v1::TelemetryMeasurement> {
        vec![
            v1::TelemetryMeasurement {
                name: "queue_depth".to_string(),
                value: 2.0,
            },
            v1::TelemetryMeasurement {
                name: "bytes".to_string(),
                value: 128.0,
            },
        ]
    }

    fn record<'a>(measurements: &'a [v1::TelemetryMeasurement]) -> TelemetryOperation<'a> {
        record_with(
            Some("agent-1"),
            v1::TelemetrySignalKind::Tool as i32,
            "telemetry_snapshot",
            v1::TelemetryOutcome::Ok as i32,
            42,
            "2026-01-01T00:00:00Z",
            measurements,
        )
    }

    fn record_with<'a>(
        agent_session_id: Option<&'a str>,
        kind: i32,
        name: &'a str,
        outcome: i32,
        duration_ms: u64,
        occurred_at: &'a str,
        measurements: &'a [v1::TelemetryMeasurement],
    ) -> TelemetryOperation<'a> {
        TelemetryOperation::Record {
            agent_session_id,
            kind,
            name,
            outcome,
            duration_ms,
            occurred_at,
            measurements,
        }
    }

    #[test]
    fn identical_telemetry_inputs_produce_identical_bytes() {
        let measurements = measurements();
        let first = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &record(&measurements),
            &authentication(1),
        );
        let second = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &record(&measurements),
            &authentication(1),
        );

        assert_eq!(first, second);
    }

    #[test]
    fn a_record_signature_binds_every_observation_field() {
        let measurements = measurements();
        let baseline = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &record(&measurements),
            &authentication(1),
        );
        let changed_measurements = vec![v1::TelemetryMeasurement {
            name: "queue_depth".to_string(),
            value: 3.0,
        }];
        let variations = [
            record_with(
                None,
                v1::TelemetrySignalKind::Tool as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Ok as i32,
                42,
                "2026-01-01T00:00:00Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Storage as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Ok as i32,
                42,
                "2026-01-01T00:00:00Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Tool as i32,
                "graph_stats",
                v1::TelemetryOutcome::Ok as i32,
                42,
                "2026-01-01T00:00:00Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Tool as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Error as i32,
                42,
                "2026-01-01T00:00:00Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Tool as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Ok as i32,
                43,
                "2026-01-01T00:00:00Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Tool as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Ok as i32,
                42,
                "2026-01-01T00:00:01Z",
                &measurements,
            ),
            record_with(
                Some("agent-1"),
                v1::TelemetrySignalKind::Tool as i32,
                "telemetry_snapshot",
                v1::TelemetryOutcome::Ok as i32,
                42,
                "2026-01-01T00:00:00Z",
                &changed_measurements,
            ),
        ];

        for variation in variations {
            assert_ne!(
                baseline,
                telemetry_signing_bytes("tenant-a", "repo-a", &variation, &authentication(1))
            );
        }
    }

    #[test]
    fn a_read_signature_binds_the_series_selection_and_bounds() {
        let baseline = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &TelemetryOperation::Read {
                kind: v1::TelemetrySignalKind::Tool as i32,
                name: Some("telemetry_snapshot"),
                bucket_seconds: 60,
                max_points: 24,
            },
            &authentication(1),
        );
        let changed_kind = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &TelemetryOperation::Read {
                kind: v1::TelemetrySignalKind::Storage as i32,
                name: Some("telemetry_snapshot"),
                bucket_seconds: 60,
                max_points: 24,
            },
            &authentication(1),
        );
        let changed_name = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &TelemetryOperation::Read {
                kind: v1::TelemetrySignalKind::Tool as i32,
                name: Some("graph_stats"),
                bucket_seconds: 60,
                max_points: 24,
            },
            &authentication(1),
        );
        let changed_bounds = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &TelemetryOperation::Read {
                kind: v1::TelemetrySignalKind::Tool as i32,
                name: Some("telemetry_snapshot"),
                bucket_seconds: 300,
                max_points: 12,
            },
            &authentication(1),
        );

        assert_ne!(baseline, changed_kind);
        assert_ne!(baseline, changed_name);
        assert_ne!(baseline, changed_bounds);
    }

    #[test]
    fn telemetry_signatures_use_a_domain_separate_from_claims_and_knowledge() {
        let measurements = measurements();
        let bytes = telemetry_signing_bytes(
            "tenant-a",
            "repo-a",
            &record(&measurements),
            &authentication(1),
        );

        assert!(bytes.starts_with(TELEMETRY_DOMAIN));
        assert_ne!(TELEMETRY_DOMAIN, crate::claim_auth::CLAIM_DOMAIN);
        assert_ne!(TELEMETRY_DOMAIN, crate::knowledge_auth::KNOWLEDGE_DOMAIN);
    }
}
