-- ADR-0134: Lifecycle purge preview and confirmation are authenticated by
-- distinct enrolled signing-key principals. Existing unsigned previews cannot
-- gain a synthetic requester identity, so they remain unconfirmable. The
-- existing claim_authentication_nonces ledger consumes this operation's
-- signed nonces; duplicating that anti-replay table here would fork its
-- enforcement rather than strengthen it.
ALTER TABLE administration_purge_requests
    ADD COLUMN IF NOT EXISTS requesting_node_id TEXT;

ALTER TABLE administration_purge_receipts
    ADD COLUMN IF NOT EXISTS confirming_signing_key_id TEXT;
ALTER TABLE administration_purge_receipts
    ADD COLUMN IF NOT EXISTS confirming_node_id TEXT;

ALTER TABLE administration_purge_requests
    DROP CONSTRAINT IF EXISTS administration_purge_requests_requesting_node_id_check;
ALTER TABLE administration_purge_requests
    ADD CONSTRAINT administration_purge_requests_requesting_node_id_check
    CHECK (
        requesting_node_id IS NULL
        OR octet_length(requesting_node_id) BETWEEN 1 AND 256
    );

ALTER TABLE administration_purge_receipts
    DROP CONSTRAINT IF EXISTS administration_purge_receipts_confirming_signing_key_id_check;
ALTER TABLE administration_purge_receipts
    ADD CONSTRAINT administration_purge_receipts_confirming_signing_key_id_check
    CHECK (
        confirming_signing_key_id IS NULL
        OR octet_length(confirming_signing_key_id) BETWEEN 1 AND 256
    );
ALTER TABLE administration_purge_receipts
    DROP CONSTRAINT IF EXISTS administration_purge_receipts_confirming_node_id_check;
ALTER TABLE administration_purge_receipts
    ADD CONSTRAINT administration_purge_receipts_confirming_node_id_check
    CHECK (
        confirming_node_id IS NULL
        OR octet_length(confirming_node_id) BETWEEN 1 AND 256
    );
