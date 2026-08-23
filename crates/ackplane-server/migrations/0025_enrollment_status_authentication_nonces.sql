-- Anti-replay for CheckEnrollmentStatus authentication (ADR-0122 decision 3):
-- a (tenant_id, repository_id, node_id, public_key_fingerprint, nonce) tuple
-- may be consumed at most once. Its own table, not
-- knowledge_authentication_nonces or any other domain's: a coincidental
-- collision in one domain must never refuse a legitimate request in another,
-- the same reasoning migrations/0006 and 0008 already establish.
--
-- Keyed on the full binding rather than just (public_key_fingerprint, nonce):
-- a fingerprint alone is not guaranteed globally unique across tenants and
-- repositories (two unrelated nodes could coincidentally register the same
-- key bytes), so the full tuple is what the signature itself binds and what
-- replay protection must therefore key on too.
CREATE TABLE IF NOT EXISTS enrollment_status_authentication_nonces (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    node_id                TEXT        NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    nonce                  BYTEA       NOT NULL,
    consumed_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, node_id, public_key_fingerprint, nonce)
);
