-- Attach the universal community write fence to the ACL projections.
--
-- Migration 0029 bootstrapped the fence over every `community_id`-carrying
-- table that existed at the time and left the contract for later migrations:
-- "future migrations must invoke this helper explicitly after CREATE/ALTER
-- introduces community_id". Migrations 0033 (project_acl,
-- project_acl_members), 0038 (coding_session_authority_acl) and 0039
-- (shell_session_acl, shell_session_acl_members) each added a scoped table
-- and none of them did.
--
-- Two things were broken as a result, in ascending order of severity:
--
--   1. `validate_catalog` compares the live fenced set against
--      EXPECTED_SCOPED_TABLES, so community deletion refused to start at all.
--      That is the fail-closed guard working correctly.
--   2. Behind it sits the actual defect: an unfenced scoped table accepts
--      writes while its community is fenced for deletion. A concurrent
--      ingest of a 30621/30623/44228 head could therefore re-project ACL
--      rows *after* the purge swept them, leaving live grants pointing at a
--      tombstoned community.
--
-- `attach_community_write_fence` is idempotent (it no-ops when the trigger
-- already exists), so this is safe on databases bootstrapped from
-- `schema/schema.sql`, where the desired-state DO-loop already attached the
-- fence to the two `project_acl*` tables it knows about.

SELECT attach_community_write_fence('coding_session_authority_acl');
SELECT attach_community_write_fence('project_acl');
SELECT attach_community_write_fence('project_acl_members');
SELECT attach_community_write_fence('shell_session_acl');
SELECT attach_community_write_fence('shell_session_acl_members');
