-- ADR-0115 upgrade: retain bounded typed operation payloads in immutable
-- events so projections can be reproduced and idempotent retries can return
-- their original outcome after a later lifecycle event.
ALTER TABLE delegation_events
    ADD COLUMN IF NOT EXISTS delegatee_session_id TEXT,
    ADD COLUMN IF NOT EXISTS project_id TEXT,
    ADD COLUMN IF NOT EXISTS task_id TEXT,
    ADD COLUMN IF NOT EXISTS goal_id TEXT,
    ADD COLUMN IF NOT EXISTS goal_digest BYTEA,
    ADD COLUMN IF NOT EXISTS policy_version TEXT,
    ADD COLUMN IF NOT EXISTS policy_digest BYTEA,
    ADD COLUMN IF NOT EXISTS constitution_version TEXT,
    ADD COLUMN IF NOT EXISTS constitution_digest BYTEA,
    ADD COLUMN IF NOT EXISTS allowed_actions SMALLINT[],
    ADD COLUMN IF NOT EXISTS max_token_budget BIGINT,
    ADD COLUMN IF NOT EXISTS max_actions_per_session BIGINT,
    ADD COLUMN IF NOT EXISTS source_protocol_version SMALLINT,
    ADD COLUMN IF NOT EXISTS effective_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS revocation_reason TEXT;

ALTER TABLE delegation_events
    ADD CONSTRAINT delegation_events_payload_shape CHECK (
        (event_kind = 1
            AND delegatee_session_id IS NOT NULL
            AND goal_id IS NOT NULL
            AND octet_length(goal_digest) = 32
            AND policy_version IS NOT NULL
            AND octet_length(policy_digest) = 32
            AND constitution_version IS NOT NULL
            AND octet_length(constitution_digest) = 32
            AND array_length(allowed_actions, 1) BETWEEN 1 AND 16
            AND max_token_budget > 0
            AND max_actions_per_session > 0
            AND source_protocol_version > 0
            AND effective_at IS NOT NULL
            AND expires_at IS NOT NULL
            AND revocation_reason IS NULL)
        OR (event_kind = 2
            AND revocation_reason IS NOT NULL
            AND delegatee_session_id IS NULL
            AND goal_id IS NULL
            AND goal_digest IS NULL
            AND policy_version IS NULL
            AND policy_digest IS NULL
            AND constitution_version IS NULL
            AND constitution_digest IS NULL
            AND allowed_actions IS NULL
            AND max_token_budget IS NULL
            AND max_actions_per_session IS NULL
            AND source_protocol_version IS NULL
            AND effective_at IS NULL
            AND expires_at IS NULL)
    ) NOT VALID;
