-- ADR-0119 decision 5: Export requests create a bounded, schema-versioned,
-- redacted representation for a named audit, portability, or legal purpose.
-- An export is not a backup and is never accepted as a restore input by
-- implication (decision 5) -- distinct tables from the Snapshot ones, no
-- shared identity space.
CREATE TABLE IF NOT EXISTS administration_export_requests (
    request_id       TEXT        NOT NULL,
    policy_id        TEXT        NOT NULL,
    requested_by     TEXT        NOT NULL,
    tenant_id        TEXT        NOT NULL,
    repository_id    TEXT        NOT NULL,
    -- 1 = TelemetryEvents. The only category this first implementation
    -- supports; more are added here, never behind a free-text field.
    data_category    SMALLINT    NOT NULL CHECK (data_category BETWEEN 1 AND 1),
    purpose          TEXT        NOT NULL,
    max_records      INTEGER     NOT NULL CHECK (max_records BETWEEN 1 AND 100000),
    idempotency_key  TEXT        NOT NULL,
    requested_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id),
    UNIQUE (requested_by, idempotency_key),
    FOREIGN KEY (policy_id) REFERENCES administration_policies (policy_id) ON DELETE RESTRICT,
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(tenant_id) BETWEEN 1 AND 256),
    CHECK (octet_length(repository_id) BETWEEN 1 AND 256),
    CHECK (octet_length(purpose) BETWEEN 1 AND 4096),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS administration_export_receipts (
    receipt_id        TEXT        NOT NULL,
    request_id        TEXT        NOT NULL,
    -- 1 = Succeeded, 2 = Failed, 3 = Refused
    outcome           SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 3),
    reason            TEXT        NOT NULL DEFAULT '',
    artifact_path     TEXT,
    manifest_digest   BYTEA CHECK (manifest_digest IS NULL OR octet_length(manifest_digest) = 32),
    schema_version    TEXT        NOT NULL,
    record_count      BIGINT CHECK (record_count IS NULL OR record_count >= 0),
    redacted_fields   TEXT[]      NOT NULL DEFAULT '{}',
    occurred_at       TIMESTAMPTZ NOT NULL,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (receipt_id),
    -- One receipt per request: a request is not retried in place, a changed
    -- attempt is a new request with a new idempotency key.
    UNIQUE (request_id),
    FOREIGN KEY (request_id) REFERENCES administration_export_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096),
    CHECK (artifact_path IS NULL OR octet_length(artifact_path) BETWEEN 1 AND 4096),
    CHECK (octet_length(schema_version) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS administration_export_receipts_by_request
    ON administration_export_receipts (request_id);
