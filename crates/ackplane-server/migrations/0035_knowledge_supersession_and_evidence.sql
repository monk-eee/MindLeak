-- ADR-0113 decisions 1, 3, 7: a knowledge statement can be superseded by a
-- newly-recorded replacement (decision 1). Supersession is distinct from
-- retirement -- the prior statement is preserved untouched (retired_at stays
-- NULL) and the reason the replacement won is receipted (decision 7), not
-- overwritten in place. `Superseded` is a third closed lifecycle_state value
-- alongside Candidate/Active (migration 0034); the CHECK constraint widens
-- accordingly.
ALTER TABLE knowledge
    DROP CONSTRAINT knowledge_lifecycle_state_check;

ALTER TABLE knowledge
    ADD CONSTRAINT knowledge_lifecycle_state_check CHECK (lifecycle_state BETWEEN 1 AND 3);

ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS superseded_by TEXT;

ALTER TABLE knowledge
    ADD CONSTRAINT knowledge_superseded_by_fkey
    FOREIGN KEY (tenant_id, repository_id, superseded_by)
    REFERENCES knowledge (tenant_id, repository_id, knowledge_id);

-- One immutable receipt per accepted Active -> Superseded transition
-- (decision 7): the prior statement's own id, the replacement's id, who
-- authorized it, and why the replacement won. Never updated or deleted once
-- written -- the same append-only contract as knowledge_reconfirmations and
-- knowledge_activations.
CREATE TABLE IF NOT EXISTS knowledge_supersessions (
    tenant_id        TEXT        NOT NULL,
    repository_id    TEXT        NOT NULL,
    knowledge_id     TEXT        NOT NULL,
    supersession_id  TEXT        NOT NULL,
    new_knowledge_id TEXT        NOT NULL,
    authorized_by    TEXT        NOT NULL,
    reason           TEXT        NOT NULL,
    superseded_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, supersession_id),
    FOREIGN KEY (tenant_id, repository_id, knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id),
    FOREIGN KEY (tenant_id, repository_id, new_knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id)
);

-- ADR-0113 decision 3: bounded evidence and outcome references, recording
-- corroboration and later contradiction as separate typed facts rather than
-- folding both into one opaque confidence number. `reference_kind` names
-- what supported the lesson (a task, a context packet, a validation run, or
-- a receipt); `polarity` names whether this particular reference corroborates
-- or contradicts the statement it is attached to. A statement can accumulate
-- evidence in any lifecycle state -- corroboration is how a Candidate earns
-- activation, and a contradiction can be recorded against Active knowledge
-- long after the fact, without that alone retiring or superseding it.
CREATE TABLE IF NOT EXISTS knowledge_evidence_references (
    tenant_id      TEXT        NOT NULL,
    repository_id  TEXT        NOT NULL,
    knowledge_id   TEXT        NOT NULL,
    reference_id   TEXT        NOT NULL,
    reference_kind SMALLINT    NOT NULL CHECK (reference_kind BETWEEN 1 AND 4),
    reference_ref  TEXT        NOT NULL,
    polarity       SMALLINT    NOT NULL CHECK (polarity BETWEEN 1 AND 2),
    recorded_by    TEXT        NOT NULL,
    recorded_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, reference_id),
    FOREIGN KEY (tenant_id, repository_id, knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id)
);

CREATE INDEX IF NOT EXISTS knowledge_evidence_references_by_knowledge
    ON knowledge_evidence_references
       (tenant_id, repository_id, knowledge_id, recorded_at DESC);
