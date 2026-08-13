//! ADR-0083's wire frames mapped onto ledger appends (ADR-0086) and back into
//! typed receipts.
//!
//! Everything here is a pure decision. It takes what a node sent and produces
//! either the storage-layer envelope the ledger accepts, or the typed rejection
//! ADR-0083 clause 6 requires: a stable reason code, the offending record's
//! identity, and whether retrying could ever succeed. Human-readable text rides
//! along as a diagnostic and is never what a client branches on.
//!
//! Nothing in this module opens a connection, so it is unit-tested with no
//! PostgreSQL, no container and no network (ADR-0088 clause 2, ADR-0091). The
//! stream that will call it is deliberately not here — this is the decision the
//! handler makes, separated from the transport that carries it.

use ackplane_protocol::v1;
use sha2::{Digest, Sha256};

use crate::ledger::{AppendError, AppendOutcome, DedupKey, EventEnvelope, ProvenanceClass};

/// The offending record's identity for ADR-0083 clause 6. An envelope carries
/// no id of its own; the dedup key *is* its identity, so this renders that key
/// in one stable, greppable form rather than inventing a second identifier.
pub fn record_identity(key: &DedupKey) -> String {
    format!(
        "{}/{}/{}@{}",
        key.tenant_id, key.repository_id, key.producer_id, key.producer_sequence
    )
}

fn malformed(identity: String, diagnostic: String) -> v1::Rejection {
    v1::Rejection {
        record_id: identity,
        reason: v1::RejectionReason::Malformed as i32,
        retryable: false,
        diagnostic,
    }
}

/// Translate a wire envelope into the record the ledger stores, or refuse it.
///
/// A refusal here is always non-retryable: every check below is a property of
/// the bytes the node sent, so resending them unchanged fails identically.
pub fn translate(wire: &v1::EventEnvelope) -> Result<EventEnvelope, v1::Rejection> {
    // Identity is reported even when the fields forming it are the problem, so
    // a malformed record is still attributable to whoever sent it.
    let identity = format!(
        "{}/{}/{}@{}",
        wire.tenant_id, wire.repository_id, wire.producer_id, wire.producer_sequence
    );

    for (label, value) in [
        ("tenant_id", &wire.tenant_id),
        ("repository_id", &wire.repository_id),
        ("producer_id", &wire.producer_id),
    ] {
        if value.is_empty() {
            return Err(malformed(identity, format!("{label} is empty")));
        }
    }

    // ADR-0083 clause 7: producer sequences are positive 63-bit values. Zero is
    // the proto3 default, so accepting it would let an unset field look like a
    // deliberate first sequence.
    let sequence = match i64::try_from(wire.producer_sequence) {
        Ok(sequence) if sequence > 0 => sequence,
        _ => {
            return Err(malformed(
                identity,
                format!(
                    "producer_sequence {} is not a positive 63-bit value",
                    wire.producer_sequence
                ),
            ))
        }
    };

    if wire.schema_version.is_empty() {
        return Err(malformed(identity, "schema_version is empty".to_string()));
    }

    // The declared digest is what the ledger compares to decide duplicate
    // versus conflict, so a node that could declare a digest unrelated to its
    // own payload would be choosing that outcome for itself. The ledger cannot
    // catch this — it only ever compares the declared digest to the stored one
    // — so it is refused here, at the boundary where the payload is in hand.
    let computed = Sha256::digest(&wire.payload);
    if wire.payload_digest != computed.as_slice() {
        return Err(malformed(
            identity,
            "payload_digest does not match the SHA-256 of payload".to_string(),
        ));
    }

    let provenance = match v1::ProvenanceClass::try_from(wire.provenance) {
        Ok(v1::ProvenanceClass::UnverifiedAttribution) => ProvenanceClass::UnverifiedAttribution,
        Ok(v1::ProvenanceClass::EnrolledNode) => ProvenanceClass::EnrolledNode,
        Ok(v1::ProvenanceClass::AuthenticatedPrincipal) => ProvenanceClass::AuthenticatedPrincipal,
        Ok(v1::ProvenanceClass::ProviderAttested) => ProvenanceClass::ProviderAttested,
        // Unspecified is the proto3 default and names no trust class; an
        // unknown number is a newer sender. Neither can be stored as a
        // specific class, and guessing one would overstate what is known.
        Ok(v1::ProvenanceClass::Unspecified) | Err(_) => {
            return Err(malformed(
                identity,
                format!("provenance {} names no trust class", wire.provenance),
            ))
        }
    };

    let occurred_at = parse_rfc3339(&wire.occurred_at).ok_or_else(|| {
        malformed(
            identity.clone(),
            format!(
                "occurred_at {:?} is not an RFC 3339 timestamp",
                wire.occurred_at
            ),
        )
    })?;

    // proto3 has no absent bytes/string, so empty is how a node says "none".
    let optional_bytes = |value: &Vec<u8>| (!value.is_empty()).then(|| value.clone());

    Ok(EventEnvelope {
        key: DedupKey {
            tenant_id: wire.tenant_id.clone(),
            repository_id: wire.repository_id.clone(),
            producer_id: wire.producer_id.clone(),
            producer_sequence: sequence,
        },
        payload: wire.payload.clone(),
        payload_digest: wire.payload_digest.clone(),
        schema_version: wire.schema_version.clone(),
        occurred_at,
        payload_type: wire.payload_type.clone(),
        previous_envelope_digest: optional_bytes(&wire.previous_envelope_digest),
        signing_key_id: (!wire.signing_key_id.is_empty()).then(|| wire.signing_key_id.clone()),
        signature: optional_bytes(&wire.signature),
        provenance,
    })
}

/// The receipt for a record the ledger accepted, carrying the position it was
/// written at. A duplicate reports the *original* position, which is what makes
/// a retry indistinguishable from the first send (ADR-0083 clause 7).
pub fn receipt(key: &DedupKey, outcome: AppendOutcome) -> v1::RecordReceipt {
    let (position, disposition) = match outcome {
        AppendOutcome::Accepted { position } => (position, v1::ReceiptDisposition::Accepted),
        AppendOutcome::Duplicate { position } => (position, v1::ReceiptDisposition::Duplicate),
    };
    v1::RecordReceipt {
        record_id: record_identity(key),
        // Positions are positive by construction (ADR-0086 clause 4).
        position: u64::try_from(position).unwrap_or(0),
        disposition: disposition as i32,
    }
}

/// Map an append failure onto the typed rejection a client can branch on.
pub fn rejection(key: &DedupKey, error: &AppendError) -> v1::Rejection {
    let (reason, retryable, diagnostic) = match error {
        AppendError::Conflict { .. } => (
            v1::RejectionReason::SequenceConflict,
            false,
            "this producer_sequence was already accepted with a different envelope digest"
                .to_string(),
        ),
        // The underlying database error is logged, never returned: it can name
        // hosts, roles and schema, and the node needs none of that to decide
        // whether to retry.
        AppendError::Database(error) => {
            tracing::error!(%error, record = %record_identity(key), "ledger append failed");
            (
                v1::RejectionReason::Unavailable,
                true,
                "the ledger was unavailable; retry with backoff".to_string(),
            )
        }
    };
    v1::Rejection {
        record_id: record_identity(key),
        reason: reason as i32,
        retryable,
        diagnostic,
    }
}

/// Parse the RFC 3339 subset a timestamp field can hold, returning `None` for
/// anything else rather than guessing.
///
/// Hand-rolled because the alternative is a new workspace dependency for one
/// field, and the root manifest is shared by every crate in the fleet.
fn parse_rfc3339(text: &str) -> Option<std::time::SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    if bytes[10] != b'T' && bytes[10] != b't' {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let mut rest = &text[19..];
    let mut nanos = 0u32;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits: String = fraction.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        // Truncate below nanosecond resolution rather than refuse the record.
        let scaled = format!("{:0<9}", &digits[..digits.len().min(9)]);
        nanos = scaled.parse().ok()?;
        rest = &rest[1 + digits.len()..];
    }

    let offset_minutes = match rest.as_bytes() {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), ..] if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let hours = rest.get(1..3)?.parse::<i64>().ok()?;
            let minutes = rest.get(4..6)?.parse::<i64>().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let magnitude = hours * 60 + minutes;
            if *sign == b'-' {
                -magnitude
            } else {
                magnitude
            }
        }
        _ => return None,
    };

    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    let epoch = std::time::UNIX_EPOCH;
    let magnitude = std::time::Duration::new(seconds.unsigned_abs(), nanos);
    if seconds >= 0 {
        epoch.checked_add(magnitude)
    } else {
        epoch.checked_sub(magnitude)
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days between 1970-01-01 and the given civil date, by Howard Hinnant's
/// `days_from_civil`. Shifting the year to start in March puts the leap day
/// last, which is what removes the special-casing.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn wire_envelope(payload: &[u8]) -> v1::EventEnvelope {
        v1::EventEnvelope {
            tenant_id: "acme".to_string(),
            repository_id: "repo".to_string(),
            producer_id: "node-1".to_string(),
            producer_sequence: 7,
            payload: payload.to_vec(),
            payload_digest: Sha256::digest(payload).to_vec(),
            schema_version: "v1".to_string(),
            occurred_at: "2026-08-13T00:00:00Z".to_string(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: Vec::new(),
            signing_key_id: String::new(),
            signature: Vec::new(),
            provenance: v1::ProvenanceClass::EnrolledNode as i32,
        }
    }

    fn key() -> DedupKey {
        DedupKey {
            tenant_id: "acme".to_string(),
            repository_id: "repo".to_string(),
            producer_id: "node-1".to_string(),
            producer_sequence: 7,
        }
    }

    #[test]
    fn a_well_formed_envelope_becomes_the_record_the_ledger_stores() {
        let wire = wire_envelope(b"fact");

        let translated = translate(&wire).expect("a well-formed envelope translates");

        assert_eq!(
            translated,
            EventEnvelope {
                key: key(),
                payload: b"fact".to_vec(),
                payload_digest: Sha256::digest(b"fact").to_vec(),
                schema_version: "v1".to_string(),
                occurred_at: UNIX_EPOCH + Duration::from_secs(1_786_579_200),
                payload_type: "structural_fact".to_string(),
                previous_envelope_digest: None,
                signing_key_id: None,
                signature: None,
                provenance: ProvenanceClass::EnrolledNode,
            }
        );
    }

    #[test]
    fn empty_optional_bytes_and_key_id_become_absent_rather_than_empty_values() {
        let mut wire = wire_envelope(b"fact");
        wire.previous_envelope_digest = vec![9, 9];
        wire.signing_key_id = "key-1".to_string();
        wire.signature = vec![1, 2];

        let translated = translate(&wire).expect("optional fields are accepted when present");

        assert_eq!(translated.previous_envelope_digest, Some(vec![9, 9]));
        assert_eq!(translated.signing_key_id, Some("key-1".to_string()));
        assert_eq!(translated.signature, Some(vec![1, 2]));
    }

    #[test]
    fn a_digest_that_does_not_match_the_payload_is_refused() {
        let mut wire = wire_envelope(b"fact");
        wire.payload_digest = Sha256::digest(b"a different payload").to_vec();

        let rejection = translate(&wire).expect_err("a mismatched digest is refused");

        assert_eq!(
            rejection,
            v1::Rejection {
                record_id: "acme/repo/node-1@7".to_string(),
                reason: v1::RejectionReason::Malformed as i32,
                retryable: false,
                diagnostic: "payload_digest does not match the SHA-256 of payload".to_string(),
            }
        );
    }

    #[test]
    fn an_absent_identity_field_is_refused_and_still_names_the_record() {
        let mut wire = wire_envelope(b"fact");
        wire.repository_id = String::new();

        let rejection = translate(&wire).expect_err("an empty repository_id is refused");

        assert_eq!(rejection.record_id, "acme//node-1@7");
        assert_eq!(rejection.reason, v1::RejectionReason::Malformed as i32);
        assert!(!rejection.retryable);
        assert_eq!(rejection.diagnostic, "repository_id is empty");
    }

    #[test]
    fn a_zero_or_oversized_producer_sequence_is_refused() {
        for sequence in [0, u64::MAX] {
            let mut wire = wire_envelope(b"fact");
            wire.producer_sequence = sequence;

            let rejection = translate(&wire).expect_err("the sequence is refused");

            assert_eq!(rejection.reason, v1::RejectionReason::Malformed as i32);
            assert_eq!(
                rejection.diagnostic,
                format!("producer_sequence {sequence} is not a positive 63-bit value")
            );
        }
    }

    #[test]
    fn an_unspecified_or_unknown_provenance_is_refused_rather_than_guessed() {
        for provenance in [v1::ProvenanceClass::Unspecified as i32, 99] {
            let mut wire = wire_envelope(b"fact");
            wire.provenance = provenance;

            let rejection = translate(&wire).expect_err("the provenance is refused");

            assert_eq!(
                rejection.diagnostic,
                format!("provenance {provenance} names no trust class")
            );
        }
    }

    #[test]
    fn an_empty_schema_version_is_refused() {
        let mut wire = wire_envelope(b"fact");
        wire.schema_version = String::new();

        let rejection = translate(&wire).expect_err("an empty schema_version is refused");

        assert_eq!(rejection.diagnostic, "schema_version is empty");
    }

    #[test]
    fn an_accepted_append_reports_the_position_it_was_written_at() {
        assert_eq!(
            receipt(&key(), AppendOutcome::Accepted { position: 42 }),
            v1::RecordReceipt {
                record_id: "acme/repo/node-1@7".to_string(),
                position: 42,
                disposition: v1::ReceiptDisposition::Accepted as i32,
            }
        );
    }

    #[test]
    fn a_duplicate_reports_the_original_position_so_a_retry_looks_like_the_first_send() {
        assert_eq!(
            receipt(&key(), AppendOutcome::Duplicate { position: 42 }),
            v1::RecordReceipt {
                record_id: "acme/repo/node-1@7".to_string(),
                position: 42,
                disposition: v1::ReceiptDisposition::Duplicate as i32,
            }
        );
    }

    #[test]
    fn a_sequence_conflict_is_reported_as_non_retryable() {
        let error = AppendError::Conflict {
            producer_id: "node-1".to_string(),
            sequence: 7,
        };

        assert_eq!(
            rejection(&key(), &error),
            v1::Rejection {
                record_id: "acme/repo/node-1@7".to_string(),
                reason: v1::RejectionReason::SequenceConflict as i32,
                retryable: false,
                diagnostic:
                    "this producer_sequence was already accepted with a different envelope digest"
                        .to_string(),
            }
        );
    }

    #[test]
    fn a_database_failure_is_retryable_and_does_not_leak_the_underlying_error() {
        let database_error = "host=localhost port=not-a-port"
            .parse::<tokio_postgres::Config>()
            .expect_err("an invalid port is a configuration error");
        let leaked = database_error.to_string();

        let rejection = rejection(&key(), &AppendError::Database(database_error));

        assert_eq!(
            rejection,
            v1::Rejection {
                record_id: "acme/repo/node-1@7".to_string(),
                reason: v1::RejectionReason::Unavailable as i32,
                retryable: true,
                diagnostic: "the ledger was unavailable; retry with backoff".to_string(),
            }
        );
        assert!(!rejection.diagnostic.contains(&leaked));
    }

    #[test]
    fn timestamps_parse_across_epochs_offsets_and_leap_days() {
        let cases = [
            ("1970-01-01T00:00:00Z", 0),
            ("2026-08-13T00:00:00Z", 1_786_579_200),
            // The same instant, written in a zone an hour ahead.
            ("2026-08-13T01:00:00+01:00", 1_786_579_200),
            ("2026-08-13T00:00:00-01:00", 1_786_582_800),
            // 2024 is a leap year, so the 29th exists and is day 60.
            ("2024-02-29T00:00:00Z", 1_709_164_800),
            ("2000-02-29T12:00:00Z", 951_825_600),
        ];

        for (text, expected) in cases {
            assert_eq!(
                parse_rfc3339(text),
                Some(UNIX_EPOCH + Duration::from_secs(expected)),
                "{text} should parse"
            );
        }
    }

    #[test]
    fn fractional_seconds_are_kept_to_nanosecond_resolution() {
        assert_eq!(
            parse_rfc3339("1970-01-01T00:00:00.5Z"),
            Some(UNIX_EPOCH + Duration::new(0, 500_000_000))
        );
        // Anything finer than a nanosecond is truncated, not refused.
        assert_eq!(
            parse_rfc3339("1970-01-01T00:00:00.1234567891Z"),
            Some(UNIX_EPOCH + Duration::new(0, 123_456_789))
        );
    }

    #[test]
    fn a_timestamp_that_is_not_a_real_instant_is_refused() {
        for text in [
            "",
            "2026-08-13",
            "2026-08-13T00:00:00",      // no zone at all
            "2026-13-01T00:00:00Z",     // month 13
            "2023-02-29T00:00:00Z",     // 2023 is not a leap year
            "2026-08-13T24:00:00Z",     // hour 24
            "2026-08-13T00:60:00Z",     // minute 60
            "2026-08-13T00:00:00+1:00", // unpadded offset
            "2026-08-13T00:00:00.Z",    // a point with no digits
            "2026/08/13T00:00:00Z",     // wrong separators
        ] {
            assert_eq!(parse_rfc3339(text), None, "{text:?} should be refused");
        }
    }

    #[test]
    fn a_malformed_timestamp_refuses_the_envelope_it_arrived_on() {
        let mut wire = wire_envelope(b"fact");
        wire.occurred_at = "yesterday".to_string();

        let rejection = translate(&wire).expect_err("a malformed timestamp is refused");

        assert_eq!(
            rejection.diagnostic,
            "occurred_at \"yesterday\" is not an RFC 3339 timestamp"
        );
    }
}
