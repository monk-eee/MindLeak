-- ADR-0145 decision 7: production recovery execution's own durable receipt,
-- distinct from the authorization-only `administration_recovery_execution_
-- confirmations` row slice 2 already writes. That row records that a second,
-- distinct enrolled key *authorized* a request; this table records what
-- actually happened when the authorized restore ran (or was refused, or
-- failed) -- the two must never be conflated, or a merely-authorized request
-- would read as an executed one.
CREATE TABLE IF NOT EXISTS administration_recovery_execution_receipts (
    receipt_id                          TEXT        NOT NULL,
    request_id                          TEXT        NOT NULL,
    tenant_id                           TEXT        NOT NULL,
    -- The pre-restore safety Snapshot's own digest (the "old" state) and the
    -- restored artifact's own digest (the "new" state) -- both already
    -- recorded on the request, restated here so the receipt is a
    -- self-contained provenance record that never needs a join to answer
    -- "what changed" (decision 7).
    old_manifest_digest                 BYTEA       NOT NULL,
    new_manifest_digest                 BYTEA       NOT NULL,
    rehearsal_id                        TEXT        NOT NULL,
    previewing_node_id                  TEXT        NOT NULL,
    previewing_public_key_fingerprint   TEXT        NOT NULL,
    confirming_node_id                  TEXT        NOT NULL,
    confirming_public_key_fingerprint   TEXT        NOT NULL,
    -- 1 = Succeeded, 2 = Failed, 3 = Refused
    outcome                             SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 3),
    reason                              TEXT        NOT NULL DEFAULT '',
    occurred_at                         TIMESTAMPTZ NOT NULL,
    recorded_at                         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (receipt_id),
    -- One receipt per request: a failed or refused attempt does not retry in
    -- place (ADR-0119 decision 10, reused here) -- a changed attempt needs a
    -- fresh preview and confirmation, and its own request id.
    UNIQUE (request_id),
    FOREIGN KEY (request_id) REFERENCES administration_recovery_execution_requests (request_id) ON DELETE CASCADE,
    FOREIGN KEY (rehearsal_id) REFERENCES administration_recovery_rehearsals (rehearsal_id) ON DELETE RESTRICT,
    CHECK (octet_length(receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(request_id) BETWEEN 1 AND 256),
    CHECK (octet_length(tenant_id) BETWEEN 1 AND 256),
    CHECK (octet_length(old_manifest_digest) = 32),
    CHECK (octet_length(new_manifest_digest) = 32),
    CHECK (octet_length(rehearsal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(previewing_node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(previewing_public_key_fingerprint) BETWEEN 1 AND 256),
    CHECK (octet_length(confirming_node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(confirming_public_key_fingerprint) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096)
);

CREATE INDEX IF NOT EXISTS administration_recovery_execution_receipts_by_request
    ON administration_recovery_execution_receipts (request_id);
