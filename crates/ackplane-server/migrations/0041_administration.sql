-- ADR-0119 decisions 2-4 and ADR-0128: adopted administration policies (the
-- authorization basis a privileged Administration operation still requires
-- even once a verified principal exists) and platform/tenant-scoped snapshot
-- requests with their immutable receipts.
CREATE TABLE IF NOT EXISTS administration_policies (
    policy_id             TEXT        NOT NULL,
    -- 1 = Snapshot, 2 = Export, 3 = RecoveryExecution, 4 = LifecyclePurge
    operation             SMALLINT    NOT NULL CHECK (operation BETWEEN 1 AND 4),
    -- 1 = Platform (tenant_id NULL), 2 = Tenant (tenant_id required)
    scope_kind            SMALLINT    NOT NULL CHECK (scope_kind BETWEEN 1 AND 2),
    tenant_id             TEXT,
    data_classification   TEXT        NOT NULL,
    retention_basis       TEXT        NOT NULL,
    adopted_by            TEXT        NOT NULL,
    idempotency_key       TEXT        NOT NULL,
    effective_at          TIMESTAMPTZ NOT NULL,
    expires_at            TIMESTAMPTZ NOT NULL,
    revoked_at            TIMESTAMPTZ,
    revoked_by            TEXT,
    recorded_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (policy_id),
    UNIQUE (adopted_by, idempotency_key),
    CHECK (octet_length(policy_id) BETWEEN 1 AND 256),
    CHECK ((scope_kind = 1 AND tenant_id IS NULL) OR (scope_kind = 2 AND tenant_id IS NOT NULL)),
    CHECK (octet_length(data_classification) BETWEEN 1 AND 256),
    CHECK (octet_length(retention_basis) BETWEEN 1 AND 4096),
    CHECK (octet_length(adopted_by) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (expires_at > effective_at),
    CHECK (revoked_by IS NULL OR octet_length(revoked_by) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS administration_policies_by_operation_scope
    ON administration_policies (operation, scope_kind, tenant_id, expires_at DESC);

CREATE TABLE IF NOT EXISTS administration_snapshot_requests (
    request_id       TEXT        NOT NULL,
    policy_id        TEXT        NOT NULL,
    requested_by     TEXT        NOT NULL,
    scope_kind       SMALLINT    NOT NULL CHECK (scope_kind BETWEEN 1 AND 2),
    tenant_id        TEXT,
    idempotency_key  TEXT        NOT NULL,
    requested_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id),
    UNIQUE (requested_by, idempotency_key),
    FOREIGN KEY (policy_id) REFERENCES administration_policies (policy_id) ON DELETE RESTRICT,
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK ((scope_kind = 1 AND tenant_id IS NULL) OR (scope_kind = 2 AND tenant_id IS NOT NULL))
);

CREATE TABLE IF NOT EXISTS administration_snapshot_receipts (
    receipt_id          TEXT        NOT NULL,
    request_id          TEXT        NOT NULL,
    -- 1 = Succeeded, 2 = Failed, 3 = Refused
    outcome             SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 3),
    reason              TEXT        NOT NULL DEFAULT '',
    artifact_path       TEXT,
    manifest_digest     BYTEA CHECK (manifest_digest IS NULL OR octet_length(manifest_digest) = 32),
    encryption_key_id   TEXT,
    size_bytes          BIGINT CHECK (size_bytes IS NULL OR size_bytes >= 0),
    verified            BOOLEAN     NOT NULL DEFAULT FALSE,
    occurred_at         TIMESTAMPTZ NOT NULL,
    recorded_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (receipt_id),
    -- One receipt per request in this first slice: a request is not retried
    -- in place, a changed attempt is a new request with a new idempotency key.
    UNIQUE (request_id),
    FOREIGN KEY (request_id) REFERENCES administration_snapshot_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096),
    CHECK (artifact_path IS NULL OR octet_length(artifact_path) BETWEEN 1 AND 4096),
    CHECK (encryption_key_id IS NULL OR octet_length(encryption_key_id) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS administration_snapshot_receipts_by_request
    ON administration_snapshot_receipts (request_id);
