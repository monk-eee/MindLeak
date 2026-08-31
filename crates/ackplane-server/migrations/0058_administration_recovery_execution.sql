-- ADR-0145 decision 4-5: production recovery execution reuses ADR-0134's
-- dual-signing-key preview/confirm workflow verbatim, scoped to Recovery.
-- This slice records the preview's explicit impact plan and the
-- confirmation's authorization outcome only -- no `pg_restore` against
-- production runs yet (slice 4). The columns mirror
-- `administration_purge_requests`/`administration_purge_receipts` exactly
-- for the same reasons those tables have them; the Recovery-specific
-- additions are the artifact/safety-snapshot/rehearsal references decision 5
-- requires and purge has no equivalent of.
CREATE TABLE IF NOT EXISTS administration_recovery_execution_requests (
    request_id                          TEXT        NOT NULL,
    policy_id                           TEXT        NOT NULL,
    requested_by                        TEXT        NOT NULL,
    -- The Bridge tenant that made this request (ADR-0098 decision 5): the
    -- recovery-execution *scope* itself is always platform-wide (decision
    -- 6), but disclosure of a request/confirmation is still bounded to the
    -- tenant that made it, exactly like `administration_purge_requests`.
    tenant_id                           TEXT        NOT NULL,
    requesting_node_id                  TEXT        NOT NULL,
    requesting_public_key_fingerprint   TEXT        NOT NULL,
    -- The Snapshot request naming the artifact being restored, and the
    -- caller-declared digest of that artifact -- cross-checked at preview
    -- time against the artifact's own recorded receipt (defense in depth: a
    -- caller cannot merely assert a digest and have it trusted).
    artifact_request_id                 TEXT        NOT NULL,
    manifest_digest                     BYTEA       NOT NULL,
    -- The fresh platform Snapshot decision 5 requires be triggered as part of
    -- preview construction -- the "one before" safety point. Its own request
    -- never has a policy exception: it is an ordinary Snapshot request and
    -- receipt like any other, just referenced here by id.
    safety_snapshot_receipt_id          TEXT        NOT NULL,
    safety_snapshot_digest              BYTEA       NOT NULL,
    -- The passing rehearsal report this preview relies on (decision 5).
    -- Freshness-window enforcement against this digest is slice 4's gate,
    -- not this slice's -- this column only records which report was named.
    rehearsal_id                        TEXT        NOT NULL,
    confirmation_expires_at             TIMESTAMPTZ NOT NULL,
    idempotency_key                     TEXT        NOT NULL,
    requested_at                        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id),
    UNIQUE (requested_by, idempotency_key),
    FOREIGN KEY (policy_id) REFERENCES administration_policies (policy_id) ON DELETE RESTRICT,
    FOREIGN KEY (artifact_request_id) REFERENCES administration_snapshot_requests (request_id) ON DELETE RESTRICT,
    FOREIGN KEY (safety_snapshot_receipt_id) REFERENCES administration_snapshot_receipts (receipt_id) ON DELETE RESTRICT,
    FOREIGN KEY (rehearsal_id) REFERENCES administration_recovery_rehearsals (rehearsal_id) ON DELETE RESTRICT,
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(policy_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requested_by) BETWEEN 1 AND 256),
    CHECK (octet_length(tenant_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requesting_node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(requesting_public_key_fingerprint) BETWEEN 1 AND 256),
    CHECK (octet_length(artifact_request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(manifest_digest) = 32),
    CHECK (octet_length(safety_snapshot_receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(safety_snapshot_digest) = 32),
    CHECK (octet_length(rehearsal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (confirmation_expires_at > requested_at)
);

-- The confirmation this slice records is an *authorization* outcome, never
-- an execution one: `Confirmed` means a second, distinct enrolled key
-- authorized this exact request, not that `pg_restore` ran against
-- production. Slice 4's own execution receipt (ADR-0145 decision 7,
-- `RecoveryExecutionReceipt`) is a distinct, later record that consumes a
-- `Confirmed` row here as its own precondition; the two must never be
-- conflated into one table, or a confirmed-but-not-yet-executed request
-- would read as if production had already changed.
CREATE TABLE IF NOT EXISTS administration_recovery_execution_confirmations (
    confirmation_id                        TEXT        NOT NULL,
    request_id                             TEXT        NOT NULL,
    -- 1 = Confirmed, 2 = Refused, 3 = Expired
    outcome                                SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 3),
    reason                                 TEXT        NOT NULL DEFAULT '',
    occurred_at                            TIMESTAMPTZ NOT NULL,
    recorded_at                            TIMESTAMPTZ NOT NULL DEFAULT now(),
    confirming_signing_key_id              TEXT,
    confirming_node_id                     TEXT,
    confirming_public_key_fingerprint      TEXT,
    PRIMARY KEY (confirmation_id),
    -- One confirmation per request: a changed attempt needs a fresh preview
    -- and its own request id, mirroring `administration_purge_receipts`.
    UNIQUE (request_id),
    FOREIGN KEY (request_id) REFERENCES administration_recovery_execution_requests (request_id) ON DELETE CASCADE,
    CHECK (octet_length(confirmation_id) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096),
    CHECK (
        confirming_signing_key_id IS NULL
        OR octet_length(confirming_signing_key_id) BETWEEN 1 AND 256
    ),
    CHECK (
        confirming_node_id IS NULL
        OR octet_length(confirming_node_id) BETWEEN 1 AND 256
    ),
    CHECK (
        confirming_public_key_fingerprint IS NULL
        OR octet_length(confirming_public_key_fingerprint) BETWEEN 1 AND 256
    )
);

CREATE INDEX IF NOT EXISTS administration_recovery_execution_confirmations_by_request
    ON administration_recovery_execution_confirmations (request_id);
