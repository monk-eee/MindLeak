-- ADR-0134 upgrade: a signing-key id alone is not a distinct credential.
-- A key can rotate to a new id while retaining identical Ed25519 material, so
-- preview and confirmation persist public-key fingerprints and compare those.
ALTER TABLE administration_purge_requests
    ADD COLUMN IF NOT EXISTS requesting_public_key_fingerprint TEXT;

ALTER TABLE administration_purge_receipts
    ADD COLUMN IF NOT EXISTS confirming_public_key_fingerprint TEXT;

ALTER TABLE administration_purge_requests
    DROP CONSTRAINT IF EXISTS administration_purge_requests_requesting_public_key_fingerprint_check;
ALTER TABLE administration_purge_requests
    ADD CONSTRAINT administration_purge_requests_requesting_public_key_fingerprint_check
    CHECK (
        requesting_public_key_fingerprint IS NULL
        OR octet_length(requesting_public_key_fingerprint) BETWEEN 1 AND 256
    );

ALTER TABLE administration_purge_receipts
    DROP CONSTRAINT IF EXISTS administration_purge_receipts_confirming_public_key_fingerprint_check;
ALTER TABLE administration_purge_receipts
    ADD CONSTRAINT administration_purge_receipts_confirming_public_key_fingerprint_check
    CHECK (
        confirming_public_key_fingerprint IS NULL
        OR octet_length(confirming_public_key_fingerprint) BETWEEN 1 AND 256
    );
