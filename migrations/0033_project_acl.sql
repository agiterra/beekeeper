-- Project access-control projection (NIP-MP Buzz access extension).
--
-- A kind:30621 project may carry ["buzz-access","private"] plus invited-member
-- `p` tags. The signed event stays authoritative (owner-curated republish);
-- these tables are a store+project projection maintained on ingest — the same
-- store+project pattern as `reactions` — so the channel accessible-set query
-- and the write-path membership check can join against project visibility
-- inside one SQL statement, pre-LIMIT.
--
-- Rows exist for EVERY live project head (public ones too): an absent row and
-- a public row both mean "not gated", but projecting both lets a deletion
-- simply drop the row and lets the LWW guard (`head_created_at`) apply
-- uniformly regardless of the head's access level.

CREATE TABLE project_acl (
    community_id    UUID   NOT NULL REFERENCES communities(id),
    owner           BYTEA  NOT NULL,  -- 32-byte project signer pubkey
    dtag            TEXT   NOT NULL,
    -- Canonical `30621:<lowercase-hex-owner>:<dtag>` — the exact string
    -- channels store in `channels.project_ref`, so the accessible-channels
    -- query joins on string equality with no per-row parsing.
    coordinate      TEXT   NOT NULL,
    visibility      TEXT   NOT NULL DEFAULT 'public'
                      CHECK (visibility IN ('public', 'private')),
    -- Replaceable-event last-write-wins guard (event seconds): a replayed
    -- stale head must never overwrite a newer one, and a stale NIP-09
    -- tombstone must never erase a newer replacement.
    head_created_at BIGINT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, owner, dtag)
);

CREATE UNIQUE INDEX idx_project_acl_coordinate
    ON project_acl (community_id, coordinate);

-- Invited members (the project event's `p` tags). The owner is an implicit
-- member and is never projected here.
CREATE TABLE project_acl_members (
    community_id UUID  NOT NULL,
    owner        BYTEA NOT NULL,
    dtag         TEXT  NOT NULL,
    pubkey       BYTEA NOT NULL,  -- 32-byte invited-member pubkey
    PRIMARY KEY (community_id, owner, dtag, pubkey),
    FOREIGN KEY (community_id, owner, dtag)
        REFERENCES project_acl (community_id, owner, dtag)
        ON DELETE CASCADE
);

CREATE INDEX idx_project_acl_members_pubkey
    ON project_acl_members (community_id, pubkey);

-- Backfill from the latest live 30621 head per (owner, d) coordinate.
-- Idempotent: re-running upserts identical rows. Existing heads predate the
-- access extension, so effectively every backfilled row is 'public' — the
-- extraction below still reads the tag so a re-run after private heads exist
-- stays correct.
INSERT INTO project_acl (community_id, owner, dtag, coordinate, visibility, head_created_at)
SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
       e.community_id,
       e.pubkey,
       e.d_tag,
       '30621:' || encode(e.pubkey, 'hex') || ':' || e.d_tag,
       CASE WHEN e.tags @> '[["buzz-access","private"]]' THEN 'private' ELSE 'public' END,
       EXTRACT(EPOCH FROM e.created_at)::bigint
  FROM events e
 WHERE e.kind = 30621
   AND e.deleted_at IS NULL
   AND e.d_tag IS NOT NULL
   AND e.d_tag <> ''
 ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC
ON CONFLICT (community_id, owner, dtag) DO UPDATE SET
    coordinate      = EXCLUDED.coordinate,
    visibility      = EXCLUDED.visibility,
    head_created_at = EXCLUDED.head_created_at,
    updated_at      = NOW()
 WHERE EXCLUDED.head_created_at >= project_acl.head_created_at;

INSERT INTO project_acl_members (community_id, owner, dtag, pubkey)
SELECT DISTINCT heads.community_id, heads.pubkey, heads.d_tag, decode(tag ->> 1, 'hex')
  FROM (
        SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
               e.community_id, e.pubkey, e.d_tag, e.tags
          FROM events e
         WHERE e.kind = 30621
           AND e.deleted_at IS NULL
           AND e.d_tag IS NOT NULL
           AND e.d_tag <> ''
         ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC
       ) heads,
       LATERAL jsonb_array_elements(heads.tags) AS tag
 WHERE tag ->> 0 = 'p'
   AND tag ->> 1 ~ '^[0-9a-f]{64}$'
ON CONFLICT DO NOTHING;
