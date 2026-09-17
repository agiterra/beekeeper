-- Host-executed workflow steps (docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md § 5.4).
--
-- A `run_on_host` step does not run on the relay. The run parks as
-- `waiting_host`, one row here records the request, exactly one host claims
-- it (the UPDATE … WHERE claimed_by IS NULL RETURNING shape, so a second host
-- loses the race and is told who won), and the claiming host's result closes
-- the row and resumes the run. `waiting_host` is a distinct run status rather
-- than a reuse of `waiting_approval` because the two wait on different
-- parties and the resume guards must tell them apart.
--
-- ADD VALUE inside a transaction is fine on PostgreSQL 12+ (the relay runs
-- 17); the new value is not used by any statement in this file.
-- Additive migration: previously applied files must not change checksum.

ALTER TYPE run_status ADD VALUE 'waiting_host';

CREATE TYPE host_step_status AS ENUM ('requested', 'claimed', 'exited', 'lost', 'expired');

CREATE TABLE workflow_host_steps (
    community_id        UUID NOT NULL REFERENCES communities(id),
    run_id              UUID NOT NULL,
    step_id             VARCHAR(64) NOT NULL,
    workflow_id         UUID NOT NULL,
    step_index          INT NOT NULL,
    status              host_step_status NOT NULL DEFAULT 'requested',
    requested_event_id  BYTEA,
    expires_at          TIMESTAMPTZ NOT NULL,
    claimed_by          BYTEA,
    claimed_at          TIMESTAMPTZ,
    claim_event_id      BYTEA,
    result_event_id     BYTEA,
    exited_event_id     BYTEA,
    exit_code           INT,
    disposition         TEXT,
    timed_out           BOOLEAN,
    duration_ms         BIGINT,
    head_sha            TEXT,
    dirty               BOOLEAN,
    artifact_ref        TEXT,
    result              JSONB,
    exited_at           TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, run_id, step_id),
    FOREIGN KEY (community_id, workflow_id)
        REFERENCES workflows (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, run_id)
        REFERENCES workflow_runs (community_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_host_steps_workflow ON workflow_host_steps (community_id, workflow_id);
CREATE INDEX idx_workflow_host_steps_status ON workflow_host_steps (community_id, status);

SELECT attach_community_write_fence('workflow_host_steps');
