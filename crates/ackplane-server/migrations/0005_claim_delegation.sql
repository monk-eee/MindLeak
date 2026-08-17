-- ADR-0096: Ackplane is the sole authority for federated task claim leases.
-- The current grant serves fast arbitration; every issued or refused decision
-- is retained in the append-only history for later reconstruction.
CREATE TABLE IF NOT EXISTS delegated_claims (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    task_id            TEXT        NOT NULL,
    owner_id           TEXT        NOT NULL,
    branch             TEXT        NOT NULL,
    claim_started_at   TIMESTAMPTZ NOT NULL,
    lease_expires_at   TIMESTAMPTZ NOT NULL,
    claim_lapses       BIGINT      NOT NULL DEFAULT 0,
    paths              TEXT[]      NOT NULL,
    symbols            TEXT[]      NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, task_id)
);

CREATE TABLE IF NOT EXISTS delegated_claim_history (
    history_id         BIGSERIAL   PRIMARY KEY,
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    task_id            TEXT        NOT NULL,
    requested_owner_id TEXT        NOT NULL,
    granted_owner_id   TEXT        NOT NULL,
    outcome            SMALLINT    NOT NULL,
    claim_started_at   TIMESTAMPTZ NOT NULL,
    lease_expires_at   TIMESTAMPTZ NOT NULL,
    claim_lapses       BIGINT      NOT NULL,
    paths              TEXT[]      NOT NULL,
    symbols            TEXT[]      NOT NULL,
    recorded_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
