-- Anti-replay for TelemetryService authentication (ADR-0105 decision 6,
-- mirroring ADR-0108's pattern): a (signing_key_id, nonce) pair may be
-- consumed at most once. Its own table, not knowledge_authentication_nonces
-- or claim_authentication_nonces: an unrelated domain's nonce collision must
-- never refuse a legitimate telemetry report. The insert's own uniqueness is
-- the enforcement -- no read-then-write race.
CREATE TABLE IF NOT EXISTS telemetry_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);
