-- Autorun grants for project actions (docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md
-- § 5.4). A kind:46030 grant with `scope: action` records one row bound to
-- the workflow's definition hash at that moment; a later run of the same
-- definition skips its approval gate while an unrevoked row matches. Editing
-- the definition changes the hash, so the row no longer matches and approval
-- is re-armed without anyone revoking anything. Revocation (kind:46032, or
-- the desktop) stamps `revoked_at`; rows are never deleted, so "who allowed
-- this, when, and who stopped it" stays answerable.
-- Additive migration: previously applied files must not change checksum.

CREATE TABLE workflow_autorun_grants (
    community_id    UUID NOT NULL REFERENCES communities(id),
    id              UUID NOT NULL DEFAULT gen_random_uuid(),
    workflow_id     UUID NOT NULL,
    definition_hash BYTEA NOT NULL,
    granted_by      BYTEA NOT NULL,
    grant_event_id  BYTEA NOT NULL,
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at      TIMESTAMPTZ,
    revoke_event_id BYTEA,
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, workflow_id)
        REFERENCES workflows (community_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_autorun_grants_workflow
    ON workflow_autorun_grants (community_id, workflow_id);

SELECT attach_community_write_fence('workflow_autorun_grants');
