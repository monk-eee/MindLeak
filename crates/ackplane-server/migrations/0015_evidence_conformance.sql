-- Tighten the first Evidence domain's session semantics. A signed node proves
-- it asserted this label, not that the label is an authenticated session.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'evidence_records'
           AND column_name = 'agent_session_id'
    ) THEN
        ALTER TABLE evidence_records
            RENAME COLUMN agent_session_id TO reported_agent_session_id;
    END IF;
END $$;

ALTER TABLE evidence_records
    ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS evidence_records_idempotency
    ON evidence_records (tenant_id, repository_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS evidence_records_by_task_recorded
    ON evidence_records (
        tenant_id,
        repository_id,
        task_id,
        recorded_at DESC,
        evidence_id ASC
    );

-- Conformance outcomes are task-bound, node-reported Evidence Board records.
-- The referenced Evidence record supplies provenance; the result carries only
-- a verdict, finding count/digest, evaluator, and derived review state rather
-- than raw finding text or a self-approved human decision.
CREATE TABLE IF NOT EXISTS conformance_records (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    conformance_id  TEXT        NOT NULL,
    task_id         TEXT        NOT NULL,
    evidence_id     TEXT        NOT NULL,
    verdict         SMALLINT    NOT NULL CHECK (verdict BETWEEN 1 AND 4),
    finding_count   BIGINT      NOT NULL CHECK (finding_count >= 0),
    findings_digest BYTEA       NOT NULL CHECK (octet_length(findings_digest) = 32),
    review_state    SMALLINT    NOT NULL CHECK (review_state BETWEEN 1 AND 3),
    reported_checked_at TIMESTAMPTZ NOT NULL,
    evaluated_by    TEXT        NOT NULL,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    idempotency_key TEXT,
    PRIMARY KEY (tenant_id, repository_id, conformance_id),
    FOREIGN KEY (tenant_id, repository_id, evidence_id)
        REFERENCES evidence_records (tenant_id, repository_id, evidence_id),
    CHECK (octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (octet_length(evidence_id) BETWEEN 1 AND 256),
    CHECK (octet_length(evaluated_by) BETWEEN 1 AND 256),
    CHECK (
        (verdict = 1 AND review_state = 1)
        OR (verdict IN (2, 4) AND review_state = 2)
        OR (verdict = 3 AND review_state = 3)
    )
);

-- Branch-local Evidence deployments may have created the first form of this
-- table before idempotency/reporting semantics were tightened. Keep those
-- records readable while making all new writes use the durable retry key.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'conformance_records'
           AND column_name = 'checked_at'
    ) THEN
        ALTER TABLE conformance_records
            RENAME COLUMN checked_at TO reported_checked_at;
    END IF;
END $$;

ALTER TABLE conformance_records
    ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS conformance_records_idempotency
    ON conformance_records (tenant_id, repository_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS conformance_records_by_task
    ON conformance_records (
        tenant_id,
        repository_id,
        task_id,
        recorded_at DESC,
        conformance_id ASC
    );
