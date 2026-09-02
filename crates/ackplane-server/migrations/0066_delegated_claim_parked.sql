-- ADR-0096 clause completion: park keeps the owner and the CAS lock over a
-- task's scope while it awaits an answer (needs_input), which the plain
-- lease-expiry check delegate()/recover() already run cannot express: a
-- parked claim deliberately clears its lease, so without this column it
-- would read as "expired" and be indistinguishable from a genuinely
-- abandoned one.
ALTER TABLE delegated_claims ADD COLUMN IF NOT EXISTS parked BOOLEAN NOT NULL DEFAULT FALSE;
