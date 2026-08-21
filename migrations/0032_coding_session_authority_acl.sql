-- Coding-session authority grants (NIP-CSAT), projected from the accepted
-- kind:44228 transition chain.
--
-- One row per live grant: `operator` (may steer: turn start/interrupt, goal
-- revisions) or `viewer` (read-only session share; confers transport-channel
-- read through the accessible-channels query). Maintained inside
-- `insert_coding_session_authority_transition_event`'s advisory-locked
-- transaction — the same lock that serializes the chain — so grant rows can
-- never race the chain head: `grant-*` upserts, `revoke` deletes. The
-- founder never has a row (foundership is the genesis signature itself).

CREATE TABLE coding_session_authority_acl (
    community_id UUID  NOT NULL REFERENCES communities(id),
    channel_id   UUID  NOT NULL,
    genesis_ref  BYTEA NOT NULL,   -- genesis (kind 44226) event id
    session_ref  TEXT  NOT NULL,   -- the genesis's csg-session umbrella UUID
    founder      BYTEA NOT NULL,   -- genesis signer (denormalized for lookups)
    grantee      BYTEA NOT NULL,
    role         TEXT  NOT NULL CHECK (role IN ('operator', 'viewer')),
    granted_seq  INT   NOT NULL,   -- chain seq of the transition that granted
    PRIMARY KEY (community_id, genesis_ref, grantee)
);

CREATE INDEX idx_cs_authority_acl_grantee
    ON coding_session_authority_acl (community_id, grantee);
CREATE INDEX idx_cs_authority_acl_channel
    ON coding_session_authority_acl (community_id, channel_id, session_ref);

-- Backfill one operator row per stored transition: every historically
-- accepted 44228 is a grant-operator (the relay refused all other types
-- before this migration), so no fold is needed — later re-grants collapse
-- via ON CONFLICT. Transitions whose genesis cannot be resolved (impossible
-- for relay-accepted rows, guarded anyway) are skipped.
INSERT INTO coding_session_authority_acl
    (community_id, channel_id, genesis_ref, session_ref, founder, grantee, role, granted_seq)
SELECT t.community_id,
       t.channel_id,
       g.id,
       (SELECT tag ->> 1
          FROM jsonb_array_elements(g.tags) AS tag
         WHERE tag ->> 0 = 'csg-session'
         LIMIT 1),
       g.pubkey,
       decode(t.content::jsonb ->> 'granteePubkey', 'hex'),
       'operator',
       (t.content::jsonb ->> 'seq')::int
  FROM events t
  JOIN events g
    ON g.community_id = t.community_id
   AND g.kind = 44226
   AND g.id = decode(
         (SELECT tag ->> 1
            FROM jsonb_array_elements(t.tags) AS tag
           WHERE tag ->> 0 = 'csat-genesis'
           LIMIT 1),
         'hex')
 WHERE t.kind = 44228
   AND t.channel_id IS NOT NULL
   AND t.content::jsonb ->> 'granteePubkey' ~ '^[0-9a-f]{64}$'
   AND (SELECT tag ->> 1
          FROM jsonb_array_elements(g.tags) AS tag
         WHERE tag ->> 0 = 'csg-session'
         LIMIT 1) IS NOT NULL
ON CONFLICT (community_id, genesis_ref, grantee) DO NOTHING;
