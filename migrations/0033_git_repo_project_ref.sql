-- Repo → project link projection (NIP-MP Buzz access extension, phase 2).
--
-- A kind:30617 repo announcement may carry a ["project", "30621:<owner>:<d>"]
-- back-reference placing the repo inside a project container. When that
-- project is private (see project_acl, migration 0032), the repo's Nostr
-- surface — the 30617 announcement, relay-signed 30618 ref state, and the
-- NIP-34 child events (patches/PRs/issues/status) that `a`-tag the repo —
-- must be hidden from readers outside the project.
--
-- The signed 30617 stays authoritative; this column is a store+project
-- projection maintained by the announcement side effect so the per-reader
-- hidden-repo query can join repo → project_acl in one SQL statement.
-- `head_created_at` is the replaceable-event LWW guard, mirroring
-- project_acl: a replayed stale head must never overwrite a newer link, and
-- a stale NIP-09 tombstone must never clear a newer one.

ALTER TABLE git_repo_names ADD COLUMN project_ref TEXT;
ALTER TABLE git_repo_names ADD COLUMN head_created_at BIGINT NOT NULL DEFAULT 0;

CREATE INDEX idx_git_repo_names_project_ref
    ON git_repo_names (community_id, project_ref)
    WHERE project_ref IS NOT NULL;

-- Backfill from the latest live 30617 head per (owner, d) coordinate.
-- Idempotent: re-running recomputes the same links. Only well-formed
-- `30621:` coordinates are projected; a malformed `project` tag stays NULL
-- (fail-open to public — those repos were publicly visible before this
-- migration, and go-forward ingest rejects malformed tags).
UPDATE git_repo_names grn
   SET project_ref     = heads.project_ref,
       head_created_at = heads.head_created_at
  FROM (
        SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
               e.community_id,
               encode(e.pubkey, 'hex') AS owner_hex,
               e.d_tag,
               (
                   SELECT tag ->> 1
                     FROM jsonb_array_elements(e.tags) AS tag
                    WHERE tag ->> 0 = 'project'
                      AND tag ->> 1 ~ '^30621:[0-9a-f]{64}:.{1,64}$'
                    LIMIT 1
               ) AS project_ref,
               EXTRACT(EPOCH FROM e.created_at)::bigint AS head_created_at
          FROM events e
         WHERE e.kind = 30617
           AND e.deleted_at IS NULL
           AND e.d_tag IS NOT NULL
           AND e.d_tag <> ''
         ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC
       ) heads
 WHERE grn.community_id = heads.community_id
   AND grn.repo_id = heads.d_tag
   AND grn.owner_pubkey = heads.owner_hex
   AND heads.project_ref IS NOT NULL;
