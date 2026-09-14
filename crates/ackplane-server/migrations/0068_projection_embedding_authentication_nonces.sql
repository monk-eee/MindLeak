CREATE TABLE IF NOT EXISTS projection_embedding_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL CHECK (octet_length(nonce) = 16),
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);
