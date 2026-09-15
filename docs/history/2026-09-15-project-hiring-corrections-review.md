# Project-agent hiring corrections review — 2026-09-15

Reviewed candidate `4c835625f` (`2379c7cf5` corrections over `42929e209`),
against `docs/PROJECT_AGENT_HIRING_IMPL.md` and the September 14 hiring and
rollout reports. This was a source-and-focused-test review only: no build,
test suite, live relay action, installation, association, fetch, or gate was
run. The previously reported gate counts were not reverified here.

## Finding

1. **Blocker — a public-to-private change does not reliably withdraw the
   world-readable association, and another host can republish it after a
   withdrawal.** `project_public` is treated as a write-once cache:
   `projects_needing_visibility` selects only records whose value is `None`
   (`desktop/src-tauri/src/managed_agents/project_association_authority.rs:268-280`),
   and `apply_visibility_results` likewise refuses to replace a known value
   (`:283-307`). The verifier is launched after workspace apply and setup
   activation, but those launches still only process unknown values
   (`desktop/src-tauri/src/commands/workspace.rs:267-273` and
   `desktop/src-tauri/src/managed_agents/project_team_setup_activation.rs:594-603`).
   Therefore an agent recorded while its project was public keeps
   `project_public == Some(true)` after the signed project head becomes private,
   and `published_project_digest` continues returning the public digest
   (`desktop/src-tauri/src/managed_agents/project_association_carry.rs:81-87`).

   The multi-host path also makes a successful withdrawal non-durable. A stale
   host deliberately never clears a carried digest when it receives the
   owner's newer digest-less 30177 (`project_association_carry.rs:98-112`), a
   behavior asserted explicitly by
   `desktop/src-tauri/src/commands/personas/inbound/carry_tests.rs:103-110` and
   the table at
   `desktop/src-tauri/src/managed_agents/project_association_carry_tests.rs:200-216`.
   On its next local publication, that host emits its carried digest
   (`project_association_carry.rs:71-87`). The publish-time guard sees the
   relay's digest-less head and proceeds rather than consulting the signed
   project visibility (`project_association_carry.rs:325-366`, specifically
   the no-digest proceed at `:360-362`). This can resurrect the public
   correlation after the host that learned the project was private withdrew
   it. It contradicts the contract's “A private project's agents are never
   announced” rule and leaves review finding 4 open.

   The correction needs freshness with a downgrade path based on the newest
   verified signed project head, plus a way for a verified private result (or
   an authenticated withdrawal derived from it) to clear/suppress carried
   publication on every host. A digest-less 30177 alone cannot distinguish a
   privacy withdrawal from the stale-host overwrite this guard was designed
   to prevent, so the two cases need provenance beyond presence/absence of the
   digest. Focused tests should cover public→private on one host and:
   host A publishes public → host B carries → A verifies private and withdraws
   → B receives/republishes, with the final relay head remaining digest-less.

## Other reviewed corrections

- Native association now reads a verified exact project head, uses the relay
  NIP-11 `self` signer for the current 39010 roster, gives a valid roster
  precedence over bootstrap `p` tags, accepts only creator/owner/collaborator,
  and refuses unreadable state
  (`desktop/src-tauri/src/managed_agents/project_association_authority.rs:82-192,
  194-264`). The command rechecks signing identity before its local write
  (`desktop/src-tauri/src/commands/agent_project_association.rs:47-63`). No
  concrete authority bypass was found in the reviewed path.
- `bee projects agents` emits `verified: true` in JSON and compact formats
  (`crates/buzz-cli/src/commands/project_agents.rs:302-323`), filters claims to
  roster-authorized authors, and has no unverified-row fallback
  (`:223-281`). Private and unreadable projects emit no discovery rows
  (`:390-450`). The original compact-output finding is closed in source.
- Incoming same-owner 30177 records and the pre-publish relay-head guard do
  carry an existing digest, and the guard fails closed on relay read failure
  (`desktop/src-tauri/src/commands/personas/inbound.rs:588-613` and
  `desktop/src-tauri/src/managed_agents/project_association_carry.rs:325-366`).
  Those mechanisms close the accidental stale-host deletion case while a
  project remains public; the blocker above is their missing privacy-change
  provenance.
- Private-project lead discovery remains dependent on the host-built first
  message. The roster builder filters current local managed-agent records by
  project and role (`desktop/src/features/coding-sessions/lib/codingSessionCrewLaunchFirstTurn.ts:23-68`),
  and the generated first turn says that this local list is the private
  project's source (`:94-137`). This preserves the fresh-start flow, but it is
  a point-in-time prompt rather than a runtime discovery surface: agents
  installed or associated after launch are absent after resume, and a new
  host cannot reconstruct another host's private roster through the CLI.
  The contract already declares private CLI discovery empty, so I did not
  classify this disclosed limitation as a second blocker for the current
  slice; it should remain explicit in rollout and continuity expectations.

## Acceptance and migration judgment

The core local selection, no-borrowing, role matching, setup backfill and
first-launch paths remain usable from the reviewed source. The privacy blocker
prevents signoff and rollout because the migration/publication flow can expose
a project-agent association after the project is private. No claim is made
here that the full acceptance matrix or migration was executed.
