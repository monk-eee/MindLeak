-- ADR-0084: an EventEnvelope carries a signing_key_id, and until this table
-- existed nothing could resolve one to a key, so every signature was
-- unverifiable however complete the surrounding shape looked.
--
-- History is preserved rather than corrected. A key that is later rotated away
-- from or revoked keeps its row and gains an end time, because an envelope
-- accepted while the key was valid must stay resolvable afterwards with the
-- status it had AT ACCEPTANCE. Overwriting or deleting would retroactively
-- invalidate evidence that was sound when it was accepted.
CREATE TABLE IF NOT EXISTS signing_keys (
    -- Opaque and supplied by the authority, deliberately NOT the fingerprint:
    -- a fingerprint identifies key material, while this identifies one binding
    -- of that material to one node for one lifetime. Rotating back to a
    -- previously held key would otherwise collide with its own history.
    signing_key_id         TEXT        PRIMARY KEY,
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    node_id                TEXT        NOT NULL,
    public_key             BYTEA       NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    activated_at           TIMESTAMPTZ NOT NULL,
    -- Open-ended until something ends it. NULL means "still in force", which is
    -- distinct from a past timestamp meaning "was in force until then".
    expires_at             TIMESTAMPTZ,
    -- Set when a successor takes over. The retiring key keeps signing in-flight
    -- records for a bounded overlap (ADR-0085 decision 7), so retirement is a
    -- point in time rather than a deletion.
    retired_at             TIMESTAMPTZ,
    revoked_at             TIMESTAMPTZ,
    revocation_reason      TEXT,
    -- One live binding of a fingerprint per node. Rotation adds a row rather
    -- than editing this one, which is what lets both overlap.
    UNIQUE (tenant_id, repository_id, node_id, public_key_fingerprint, activated_at)
);

-- Verification resolves by signing_key_id, but revocation and rotation sweeps
-- ask "which keys does this node hold", so both are indexed.
CREATE INDEX IF NOT EXISTS idx_signing_keys_node
    ON signing_keys (tenant_id, repository_id, node_id);
