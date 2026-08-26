-- ADR-0119 decision 7's migration key 50 self-approval fix: "An agent or model cannot
-- approve its own purge." `confirm_purge` previously had no separation of
-- duties -- only that the confirming caller was the same tenant/principal
-- that submitted the preview, which is the opposite of what decision 7
-- requires. This adds the nullable column that records the distinct,
-- attributed confirming label a valid confirmation supplies (ADR-0071's
-- reviewer-label guard, applied here). A same-label or empty-label attempt
-- is refused before any receipt is written (administration_purge_receipts
-- allows only one receipt per request, ever, so an invalid attempt must not
-- consume it) and so never reaches this column.
ALTER TABLE administration_purge_receipts
    ADD COLUMN IF NOT EXISTS confirming_label TEXT;

ALTER TABLE administration_purge_receipts
    DROP CONSTRAINT IF EXISTS administration_purge_receipts_confirming_label_check;
ALTER TABLE administration_purge_receipts
    ADD CONSTRAINT administration_purge_receipts_confirming_label_check
    CHECK (confirming_label IS NULL OR octet_length(confirming_label) BETWEEN 1 AND 256);
