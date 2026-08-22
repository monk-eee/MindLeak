-- Anti-replay for ConstitutionService authentication: a (signing_key_id,
-- nonce) pair may be consumed at most once. Its own table, not
-- knowledge_authentication_nonces or claim_authentication_nonces: an
-- unrelated domain's nonce budget must not couple to this one's, and a
-- coincidental cross-domain collision must never refuse a legitimate
-- request. The insert's own uniqueness is the enforcement -- no
-- read-then-write race -- the same pattern the other two nonce tables use.
CREATE TABLE IF NOT EXISTS constitution_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);
