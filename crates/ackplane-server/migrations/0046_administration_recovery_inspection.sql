-- ADR-0119 decision 6: recovery inspection is a read-only report against an
-- identified Snapshot artifact -- integrity, decryptability, and archive
-- validity -- and never mutates the authoritative production database.
CREATE TABLE IF NOT EXISTS administration_recovery_inspections (
    inspection_id        TEXT        NOT NULL,
    request_id            TEXT        NOT NULL,
    requested_by           TEXT        NOT NULL,
    integrity_verified      BOOLEAN     NOT NULL,
    decryption_verified     BOOLEAN     NOT NULL,
    archive_valid           BOOLEAN     NOT NULL,
    archive_entry_count     BIGINT CHECK (archive_entry_count IS NULL OR archive_entry_count >= 0),
    reason                  TEXT        NOT NULL DEFAULT '',
    occurred_at             TIMESTAMPTZ NOT NULL,
    recorded_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (inspection_id),
    FOREIGN KEY (request_id) REFERENCES administration_snapshot_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(inspection_id) BETWEEN 1 AND 256),
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096)
);

CREATE INDEX IF NOT EXISTS administration_recovery_inspections_by_request
    ON administration_recovery_inspections (request_id, recorded_at DESC);
