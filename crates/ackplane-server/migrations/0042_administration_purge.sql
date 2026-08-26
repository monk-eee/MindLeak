-- ADR-0119 decisions 1, 7, 9 and ADR-0128: Lifecycle purge is a separately
-- named, refusal-first, two-phase (preview then confirm) destructive
-- operation against one closed data category, never a generic reset. The
-- receipt retains only redacted, bounded audit metadata -- request/receipt
-- identities, scope, authorizing basis, time, and result -- never the
-- purged payload itself.
CREATE TABLE IF NOT EXISTS administration_purge_requests (
    request_id                TEXT        NOT NULL,
    policy_id                 TEXT        NOT NULL,
    requested_by              TEXT        NOT NULL,
    tenant_id                 TEXT        NOT NULL,
    repository_id             TEXT        NOT NULL,
    -- 1 = TelemetryEvents. The only category this first implementation
    -- supports; more are added here, never behind a free-text field.
    data_category              SMALLINT    NOT NULL CHECK (data_category BETWEEN 1 AND 1),
    older_than                 TIMESTAMPTZ NOT NULL,
    preview_row_count          BIGINT      NOT NULL CHECK (preview_row_count >= 0),
    confirmation_expires_at    TIMESTAMPTZ NOT NULL,
    idempotency_key            TEXT        NOT NULL,
    requested_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id),
    UNIQUE (requested_by, idempotency_key),
    FOREIGN KEY (policy_id) REFERENCES administration_policies (policy_id) ON DELETE RESTRICT,
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(tenant_id) BETWEEN 1 AND 256),
    CHECK (octet_length(repository_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (confirmation_expires_at > requested_at)
);

CREATE TABLE IF NOT EXISTS administration_purge_receipts (
    receipt_id       TEXT        NOT NULL,
    request_id       TEXT        NOT NULL,
    -- 1 = Succeeded, 2 = Failed, 3 = Refused, 4 = Expired
    outcome          SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 4),
    reason           TEXT        NOT NULL DEFAULT '',
    rows_deleted     BIGINT CHECK (rows_deleted IS NULL OR rows_deleted >= 0),
    occurred_at      TIMESTAMPTZ NOT NULL,
    recorded_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (receipt_id),
    -- One receipt per request: a confirmation is not retried in place, a
    -- changed attempt needs a fresh preview and its own request id.
    UNIQUE (request_id),
    FOREIGN KEY (request_id) REFERENCES administration_purge_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096)
);

CREATE INDEX IF NOT EXISTS administration_purge_receipts_by_request
    ON administration_purge_receipts (request_id);
