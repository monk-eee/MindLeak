CREATE TABLE IF NOT EXISTS industrial_designs (
    tenant_id               TEXT        NOT NULL,
    repository_id           TEXT        NOT NULL,
    design_id               TEXT        NOT NULL,
    title                   TEXT        NOT NULL,
    summary                 TEXT        NOT NULL,
    source_version          TEXT        NOT NULL,
    lifecycle_state         SMALLINT    NOT NULL,
    constitution_version_id TEXT,
    evidence_id             TEXT,
    content_digest          BYTEA       NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, design_id),
    CONSTRAINT industrial_designs_title_check
        CHECK (octet_length(title) >= 1 AND octet_length(title) <= 512),
    CONSTRAINT industrial_designs_summary_check
        CHECK (octet_length(summary) <= 8192),
    CONSTRAINT industrial_designs_source_version_check
        CHECK (octet_length(source_version) >= 1 AND octet_length(source_version) <= 64),
    CONSTRAINT industrial_designs_lifecycle_state_check
        CHECK (lifecycle_state >= 1 AND lifecycle_state <= 7),
    CONSTRAINT industrial_designs_content_digest_check
        CHECK (octet_length(content_digest) = 32),
    CONSTRAINT industrial_designs_constitution_version_fkey
        FOREIGN KEY (tenant_id, repository_id, constitution_version_id)
        REFERENCES constitution_publications (tenant_id, repository_id, version_id),
    CONSTRAINT industrial_designs_evidence_fkey
        FOREIGN KEY (tenant_id, repository_id, evidence_id)
        REFERENCES evidence_records (tenant_id, repository_id, evidence_id)
);

-- Append-only: one row per lifecycle state transition a design has gone
-- through, starting with the proposal recorded at creation (ADR-0121
-- decision 3). `decision_kind` shares `industrial_designs.lifecycle_state`'s
-- vocabulary; nothing here rewrites or deletes an earlier row.
CREATE TABLE IF NOT EXISTS industrial_design_decisions (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    design_id       TEXT        NOT NULL,
    sequence_number BIGINT      NOT NULL,
    decision_kind   SMALLINT    NOT NULL,
    actor           TEXT        NOT NULL,
    rationale       TEXT,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, design_id, sequence_number),
    CONSTRAINT industrial_design_decisions_actor_check
        CHECK (octet_length(actor) >= 1 AND octet_length(actor) <= 256),
    CONSTRAINT industrial_design_decisions_rationale_check
        CHECK (rationale IS NULL OR octet_length(rationale) <= 8192),
    CONSTRAINT industrial_design_decisions_kind_check
        CHECK (decision_kind >= 1 AND decision_kind <= 7),
    CONSTRAINT industrial_design_decisions_design_fkey
        FOREIGN KEY (tenant_id, repository_id, design_id)
        REFERENCES industrial_designs (tenant_id, repository_id, design_id)
);
