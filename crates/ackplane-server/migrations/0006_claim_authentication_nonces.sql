-- Anti-replay for ClaimDelegationService authentication (ADR-0096 clause 4's
-- authentication gap; gaps.d/claim-authentication-can-be-replayed-across-operations.md):
-- a (signing_key_id, nonce) pair may be consumed at most once. The insert's
-- own uniqueness is the enforcement -- no read-then-write race -- exactly the
-- pattern activation_challenges' nonce column already uses for the enrolment
-- ceremony (migrations/0003_enrollment.sql).
CREATE TABLE IF NOT EXISTS claim_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);
