-- Anti-replay for KnowledgeService authentication (ADR-0108 decision 3): a
-- (signing_key_id, nonce) pair may be consumed at most once. Its own table,
-- not claim_authentication_nonces: a knowledge nonce and a claim nonce are
-- unrelated pairs, and sharing one table would let a coincidental collision
-- in one domain refuse a legitimate request in the other. The insert's own
-- uniqueness is the enforcement -- no read-then-write race -- the same
-- pattern claim_authentication_nonces already uses (migrations/0006).
CREATE TABLE IF NOT EXISTS knowledge_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);
