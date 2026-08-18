-- Shared-terminal roster projection (NIP-ST).
--
-- Store+project projection of kind:30623 announce heads, keyed by
-- (community, owner, session id) — the same LWW `head_created_at` guard as
-- project_acl (0033). The owner-signed announce stays authoritative; these
-- rows let the ephemeral input/watch gates resolve "is this sender an
-- invited collaborator/viewer of this terminal" without parsing the stored
-- head per event. Rosters ride the announce's `p` tags
-- (["p", <hex>, <hint>, <role>], role IN collaborator|viewer); the owner is
-- implicit and never listed. Existing heads carry no roster tags, so the
-- backfill seeds empty rosters (rows only).

CREATE TABLE shell_session_acl (
    community_id    UUID   NOT NULL REFERENCES communities(id),
    owner           BYTEA  NOT NULL,  -- 32-byte announce signer pubkey
    session_id      TEXT   NOT NULL,  -- 30623 d tag
    coordinate      TEXT   NOT NULL,  -- project coordinate (a tag)
    status          TEXT   NOT NULL,  -- open | closed
    head_created_at BIGINT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, owner, session_id)
);

CREATE TABLE shell_session_acl_members (
    community_id UUID  NOT NULL,
    owner        BYTEA NOT NULL,
    session_id   TEXT  NOT NULL,
    pubkey       BYTEA NOT NULL,
    role         TEXT  NOT NULL CHECK (role IN ('collaborator', 'viewer')),
    PRIMARY KEY (community_id, owner, session_id, pubkey),
    FOREIGN KEY (community_id, owner, session_id)
        REFERENCES shell_session_acl (community_id, owner, session_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_shell_session_acl_members_pubkey
    ON shell_session_acl_members (community_id, pubkey);

-- Backfill from the latest live 30623 head per (owner, d). Pre-roster heads
-- have no role-tagged p tags, so only the session rows land.
INSERT INTO shell_session_acl (community_id, owner, session_id, coordinate, status, head_created_at)
SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
       e.community_id,
       e.pubkey,
       e.d_tag,
       COALESCE((SELECT tag ->> 1 FROM jsonb_array_elements(e.tags) AS tag
                  WHERE tag ->> 0 = 'a' LIMIT 1), ''),
       COALESCE((SELECT tag ->> 1 FROM jsonb_array_elements(e.tags) AS tag
                  WHERE tag ->> 0 = 'status' LIMIT 1), 'open'),
       EXTRACT(EPOCH FROM e.created_at)::bigint
  FROM events e
 WHERE e.kind = 30623
   AND e.deleted_at IS NULL
   AND e.d_tag IS NOT NULL
   AND e.d_tag <> ''
 ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC
ON CONFLICT (community_id, owner, session_id) DO NOTHING;

INSERT INTO shell_session_acl_members (community_id, owner, session_id, pubkey, role)
SELECT DISTINCT heads.community_id, heads.pubkey, heads.d_tag,
       decode(tag ->> 1, 'hex'), tag ->> 3
  FROM (
        SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
               e.community_id, e.pubkey, e.d_tag, e.tags
          FROM events e
         WHERE e.kind = 30623
           AND e.deleted_at IS NULL
           AND e.d_tag IS NOT NULL
           AND e.d_tag <> ''
         ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC
       ) heads,
       LATERAL jsonb_array_elements(heads.tags) AS tag
 WHERE tag ->> 0 = 'p'
   AND tag ->> 1 ~ '^[0-9a-f]{64}$'
   AND tag ->> 3 IN ('collaborator', 'viewer')
ON CONFLICT DO NOTHING;
