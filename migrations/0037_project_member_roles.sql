-- Project member roles (NIP-MP Buzz access extension, phase 3).
--
-- Adds a role dimension to the flat project invite list. Existing members
-- backfill to 'collaborator': pre-role members could already write into
-- project contents, which is exactly the Collaborator capability set —
-- 'viewer' would silently strip capabilities, 'owner' would silently grant
-- roster control.
--
-- roster_source tracks which authority maintains a project's roster:
--   'head' — the creator-signed kind:30621 head's p tags (legacy + bootstrap;
--            every republish replaces the member set, today's semantics).
--   'ops'  — relay-managed membership ops (kind 9010/9011). Flipped by the
--            first accepted op and never flipped back: from then on head
--            p tags are ignored by the projection, so a stale creator-signed
--            head replay cannot evict members added by a co-owner.

ALTER TABLE project_acl_members
    ADD COLUMN role TEXT NOT NULL DEFAULT 'collaborator'
    CHECK (role IN ('owner', 'collaborator', 'viewer'));

ALTER TABLE project_acl
    ADD COLUMN roster_source TEXT NOT NULL DEFAULT 'head'
    CHECK (roster_source IN ('head', 'ops'));
