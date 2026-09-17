-- Project-scoped workflow definitions (docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md
-- § 5.3). A definition published from a project's `beekeeper/actions.yml`
-- carries the project's kind:30621 coordinate; `ref_updated` triggers use it
-- to find the workflows a pushed repository belongs to, and admission for a
-- project-scoped write follows the kind:30624 rule (project creator, roster
-- owner, or repository founder) rather than channel role alone.
--
-- Plain TEXT with no foreign key, like `channels.project_ref` (0032): the
-- coordinate's shape is validated at ingest and resolved by clients.
-- Additive migration: previously applied files must not change checksum.
ALTER TABLE workflows ADD COLUMN project_ref TEXT;

CREATE INDEX idx_workflows_project ON workflows (community_id, project_ref)
    WHERE project_ref IS NOT NULL;
