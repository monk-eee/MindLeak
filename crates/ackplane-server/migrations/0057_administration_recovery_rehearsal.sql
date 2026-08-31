-- ADR-0145 decision 1-2: recovery rehearsal is a real restore drill against an
-- isolated, ephemeral target -- never the authoritative production database --
-- whose durable report is the compatibility and integrity evidence production
-- recovery execution (a later slice) requires before it may run at all. Unlike
-- a Snapshot or purge request, a rehearsal is not deduplicated by idempotency
-- key: two rehearsals of the same artifact are legitimately distinct events,
-- not replays of one another, exactly like `administration_recovery_inspections`.
CREATE TABLE IF NOT EXISTS administration_recovery_rehearsals (
    rehearsal_id            TEXT        NOT NULL,
    request_id               TEXT        NOT NULL,
    requested_by               TEXT        NOT NULL,
    manifest_digest            BYTEA       NOT NULL,
    restore_duration_ms        BIGINT      NOT NULL CHECK (restore_duration_ms >= 0),
    migration_version_matched  BOOLEAN     NOT NULL,
    archive_table_count        BIGINT CHECK (archive_table_count IS NULL OR archive_table_count >= 0),
    restored_table_count       BIGINT CHECK (restored_table_count IS NULL OR restored_table_count >= 0),
    restored_row_count         BIGINT CHECK (restored_row_count IS NULL OR restored_row_count >= 0),
    passed                     BOOLEAN     NOT NULL,
    reason                     TEXT        NOT NULL DEFAULT '',
    occurred_at                TIMESTAMPTZ NOT NULL,
    recorded_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (rehearsal_id),
    FOREIGN KEY (request_id) REFERENCES administration_snapshot_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(rehearsal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(manifest_digest) = 32),
    CHECK (octet_length(reason) <= 4096)
);

-- Slice 3/4's freshness gate looks up the most recent *passing* rehearsal for
-- an exact artifact digest, not merely the most recent request -- a digest can
-- theoretically be re-snapshotted under the same request in principle, and a
-- stale passing rehearsal of a *different* digest must not satisfy freshness.
CREATE INDEX IF NOT EXISTS administration_recovery_rehearsals_by_digest
    ON administration_recovery_rehearsals (manifest_digest, recorded_at DESC);

CREATE INDEX IF NOT EXISTS administration_recovery_rehearsals_by_request
    ON administration_recovery_rehearsals (request_id, recorded_at DESC);
