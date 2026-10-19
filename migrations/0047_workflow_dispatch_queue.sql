-- Durable workflow dispatch for kind:50000 application events and kind:46008
-- CI results (GitHub-plugin slice, plan § 4-5).
--
-- Today a workflow trigger is a `tokio::spawn` after the event commits: a
-- crash between commit and spawn loses the trigger, and kind:46008 never
-- reaches the engine at all (it is a workflow-execution kind, and it is
-- stored without a channel). The AFTER INSERT trigger below writes one queue
-- row in the transaction that makes the event durable, so every committed
-- 50000/46008 has a crash-safe dispatch job and a rolled-back insert never
-- does. A worker claims rows, freezes the candidate workflows into
-- `workflow_dispatch_outcomes`, and admits each one at most once.
--
-- Exactly-once run creation does not use a unique key on `trigger_event_id`:
-- `ref_updated` legitimately creates one run per changed ref with the same
-- trigger_event_id. A separate nullable `dispatch_event_id`, written only by
-- the queue, carries a PARTIAL unique index; historical rows stay NULL, so
-- building the index cannot fail on old duplicates.
--
-- Rows are never deleted on exhaustion or on a missing source event: they go
-- to state 'failed' with a reason, so a dispatch that did not happen stays
-- visible. Retention of 'done'/'failed' rows is the worker's sweep.
--
-- Keep the kind list in enqueue_workflow_dispatch() identical to
-- beekeeper_db::workflow_dispatch::DISPATCH_KINDS (a unit test pins it).
-- Additive migration: previously applied files must not change checksum.

CREATE TABLE workflow_dispatch_queue (
    community_id      UUID NOT NULL REFERENCES communities(id),
    event_id          BYTEA NOT NULL CHECK (length(event_id) = 32),
    kind              INTEGER NOT NULL,
    state             TEXT NOT NULL DEFAULT 'pending'
                          CHECK (state IN ('pending', 'claimed', 'done', 'failed')),
    attempts          INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until       TIMESTAMPTZ,
    claim_id          UUID,
    candidates_frozen BOOLEAN NOT NULL DEFAULT false,
    last_error        TEXT,
    terminal_reason   TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at       TIMESTAMPTZ,
    PRIMARY KEY (community_id, event_id)
);
CREATE INDEX workflow_dispatch_queue_due
    ON workflow_dispatch_queue (next_attempt_at, created_at) WHERE state = 'pending';
CREATE INDEX workflow_dispatch_queue_recovery
    ON workflow_dispatch_queue (lease_until) WHERE state = 'claimed';
CREATE INDEX workflow_dispatch_queue_failed
    ON workflow_dispatch_queue (community_id, finished_at) WHERE state = 'failed';

CREATE TABLE workflow_dispatch_outcomes (
    community_id UUID NOT NULL REFERENCES communities(id),
    event_id     BYTEA NOT NULL,
    workflow_id  UUID NOT NULL,
    disposition  TEXT NOT NULL DEFAULT 'pending'
                     CHECK (disposition IN ('pending', 'admitted', 'skipped', 'failed')),
    run_id       UUID,
    reason       TEXT,
    attempts     INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, event_id, workflow_id),
    FOREIGN KEY (community_id, event_id)
        REFERENCES workflow_dispatch_queue (community_id, event_id) ON DELETE CASCADE,
    CHECK ((disposition = 'admitted') = (run_id IS NOT NULL))
);
CREATE INDEX workflow_dispatch_outcomes_workflow
    ON workflow_dispatch_outcomes (community_id, workflow_id, updated_at);

ALTER TABLE workflow_runs
    ADD COLUMN dispatch_event_id BYTEA
        CHECK (dispatch_event_id IS NULL OR length(dispatch_event_id) = 32);
CREATE UNIQUE INDEX workflow_runs_dispatch_once
    ON workflow_runs (community_id, workflow_id, dispatch_event_id)
    WHERE dispatch_event_id IS NOT NULL;

CREATE FUNCTION enqueue_workflow_dispatch() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.kind IN (50000, 46008) THEN
        INSERT INTO workflow_dispatch_queue (community_id, event_id, kind)
        VALUES (NEW.community_id, NEW.id, NEW.kind)
        ON CONFLICT DO NOTHING;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER events_enqueue_workflow_dispatch
AFTER INSERT ON events
FOR EACH ROW EXECUTE FUNCTION enqueue_workflow_dispatch();

SELECT attach_community_write_fence('workflow_dispatch_queue');
SELECT attach_community_write_fence('workflow_dispatch_outcomes');
