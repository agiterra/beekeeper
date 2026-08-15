-- Retype existing session-transport channels to channel_type 'transport'.
--
-- Before the transport type existed, the desktop created these as private
-- 'stream' channels named "<project> sessions". Keying the backfill on
-- actual coding-session traffic (442xx events) plus a project ref — never
-- on the display name — guarantees a user-named chat channel that happens
-- to match the naming convention is left untouched.
UPDATE channels
SET channel_type = 'transport'
WHERE channel_type = 'stream'
  AND visibility = 'private'
  AND project_ref IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM events e
      WHERE e.community_id = channels.community_id
        AND e.channel_id = channels.id
        AND e.kind BETWEEN 44220 AND 44225
  );
