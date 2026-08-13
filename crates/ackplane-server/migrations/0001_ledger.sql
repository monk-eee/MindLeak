-- ADR-0086: the ledger schema and its idempotent append transaction.
--
-- Positions are per (tenant_id, repository_id) stream (clause 4): locking one
-- stream head serialises records within that stream without serialising
-- unrelated repositories. `stream_heads` holds the highest position assigned
-- so far; the append transaction locks its row via `UPDATE ... RETURNING`
-- before allocating the next one.
CREATE TABLE IF NOT EXISTS stream_heads (
    tenant_id     TEXT   NOT NULL,
    repository_id TEXT   NOT NULL,
    position      BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, repository_id)
);

-- The ADR-0083 deduplication key (tenant_id, repository_id, producer_id,
-- producer_sequence) is enforced here as the primary key, not merely a unique
-- index: it is the record's identity, and there is no other id a retry could
-- look itself up by (clause 5). Accepted records are immutable (clause 6);
-- ordinary application roles must not be granted UPDATE or DELETE here.
CREATE TABLE IF NOT EXISTS ledger_records (
    tenant_id                 TEXT        NOT NULL,
    repository_id             TEXT        NOT NULL,
    producer_id               TEXT        NOT NULL,
    producer_sequence         BIGINT      NOT NULL,
    stream_position            BIGINT      NOT NULL,
    payload                   BYTEA       NOT NULL,
    payload_digest             BYTEA       NOT NULL,
    schema_version             TEXT        NOT NULL,
    occurred_at                 TIMESTAMPTZ NOT NULL,
    payload_type                TEXT        NOT NULL,
    previous_envelope_digest     BYTEA,
    signing_key_id              TEXT,
    signature                  BYTEA,
    provenance_class            SMALLINT    NOT NULL,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, producer_id, producer_sequence),
    UNIQUE (tenant_id, repository_id, stream_position)
);

-- A receipt is stored in the same transaction as its record (clause 11), so a
-- same-key/same-digest retry can read it back without recomputing anything.
-- Keyed identically to `ledger_records` so a retry looks itself up the same
-- way regardless of which table answers.
CREATE TABLE IF NOT EXISTS ledger_receipts (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    producer_id        TEXT        NOT NULL,
    producer_sequence  BIGINT      NOT NULL,
    stream_position     BIGINT      NOT NULL,
    disposition        SMALLINT    NOT NULL,
    payload_digest      BYTEA       NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, producer_id, producer_sequence)
);
