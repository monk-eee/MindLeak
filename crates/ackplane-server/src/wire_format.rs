//! Shared wire-format helpers used by every gRPC service module.

use std::time::SystemTime;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Format a `SystemTime` as RFC3339, the wire format every service module
/// already uses for gRPC-visible timestamps.
///
/// Callers map the formatting error into their own error type and message --
/// each module's error shape differs deliberately (a `tonic::Status`, a
/// domain error enum, or a module-specific `String` describing which field
/// failed to format), so this returns the raw `time::error::Format` rather
/// than picking one shape for all of them. This is the conversion nine
/// service modules had each defined identically; see
/// gaps.d/ackplane-server-rfc3339-helper-duplicated-nine-times.md.
pub(crate) fn rfc3339(timestamp: SystemTime) -> Result<String, time::error::Format> {
    OffsetDateTime::from(timestamp).format(&Rfc3339)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_unix_epoch_as_rfc3339() {
        assert_eq!(
            rfc3339(SystemTime::UNIX_EPOCH).unwrap(),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn formats_a_later_timestamp_as_rfc3339() {
        let timestamp = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(rfc3339(timestamp).unwrap(), "2023-11-14T22:13:20Z");
    }
}
