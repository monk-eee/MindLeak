-- ADR-0146 decision 3: the server records the highest supervisor-declared
-- outbox sequence it has durably accepted, per
-- (tenant_id, repository_id, supervisor_id), and never derives it.
--
-- This table exists so that ADR-0141's "the server reports its OWN position"
-- is literally true. The tempting alternative -- counting the supervisor
-- frames the server has accepted -- needs no table at all, which is precisely
-- why it looks attractive and why ADR-0146 rejects it: a count matches the
-- supervisor's sequence only through an unstated invariant (that the outbox
-- contains exactly directive receipts, forever), and diverges silently the
-- moment ADR-0135's outbox carries another frame type. Storing what the
-- supervisor actually asserted cannot drift from what the supervisor actually
-- asserted.
--
-- `accepted_sequence` advances only upward and only on durable acceptance of a
-- frame that carried a sequence. A frame with no sequence advances nothing, so
-- there is deliberately no DEFAULT that a sequence-less frame could trip.
--
-- BIGINT rather than NUMERIC: the wire type is uint64, and PostgreSQL has no
-- unsigned integer. Values above i64::MAX are refused in Rust at the
-- conversion boundary rather than silently wrapping into a negative sequence,
-- which the CHECK below would then reject anyway -- belt and braces, because a
-- negative "highest accepted" would be reported to a supervisor as evidence.
--
-- The foreign key is what makes the (tenant, repository, supervisor) triple
-- meaningful: a position for a supervisor that never registered would be a
-- position for nobody. ON DELETE CASCADE keeps it from outliving the
-- registration it describes.
CREATE TABLE IF NOT EXISTS supervisor_outbox_positions (
    tenant_id         TEXT        NOT NULL,
    repository_id     TEXT        NOT NULL,
    supervisor_id     TEXT        NOT NULL,
    accepted_sequence BIGINT      NOT NULL CHECK (accepted_sequence >= 0),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, supervisor_id),
    FOREIGN KEY (tenant_id, repository_id, supervisor_id)
        REFERENCES supervisor_registrations (tenant_id, repository_id, supervisor_id)
        ON DELETE CASCADE
);
