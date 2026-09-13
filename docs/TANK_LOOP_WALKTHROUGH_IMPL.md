# Tank Loop setup walkthrough — implementation contract (2026-09-13)

Binding for the lanes fixing Brian's live Tank Loop setup walkthrough. Base:
`main` `5463b08f5`. Worktree `review-tank-loop-walkthrough-opus`, branch
`work/tank-loop-walkthrough-opus`. Findings and live evidence go into the
dated report under `docs/history/`, not here.

Product intent: a project owns shared role instructions and skills; its lead
selects workers by task; ordinary Solo sessions stay available; nobody has to
assemble a fixed team before starting a lead.

Hard rules for every lane:

- Never bypass authorization or auto-grant access. The relay decides; the
  client mirrors the relay's rule and says honestly when it cannot tell.
- Preserve drafts, published packs, installed identities (Loom
  `1c47d440…` and its name), saved teams and sessions. No deletion of user
  data. No live Tank Loop changes, no replacement packs, no restarts.
- Do not change the native setup authoring brief text
  (`project_team_setup_authoring.rs` `authoring_prompt`): `ensure_brief`
  refuses a saved draft whose `PROJECT_TEAM_SETUP.md` no longer matches, so a
  text change breaks existing drafts, including Tank Loop's.
- Lanes never commit, stage, push or install. One finalizer commits.

## Confirmed causes (evidence in the dated report)

1. **Founder view-only.** "Project State Planning" runs in the project
   transport channel `6620be79` created by Andy on 2026-08-31. Brian is a
   project **owner** but has no channel-membership row. The relay admits a
   transport channel's project owner and write-capable members
   (owner/collaborator) for coding-session writes without membership
   (`crates/buzz-relay/src/handlers/ingest.rs:946-958`); the workspace
   composer checks only `channel.isMember`
   (`CodingSessionWorkspace.tsx:204`). A false "view only".
2. **Installed agents missing from a project filter.** The filter reads only
   `seatProjectIds` (`agentDirectoryModel.ts:248`). Installation is recorded
   per project only in the setup publication journal.
3. **Ambiguous lead picker.** "Who leads" lists every managed agent with a role
   and no project filter (`useCodingSessionFoundedSetup.ts:242-261`,
   `NewCodingSessionLeadField.tsx:43-67`).
4. **Rename hidden.** `update_managed_agent {pubkey, name}` already renames in
   place (same pubkey, role untouched, kind:0 republished); no setup or
   project surface exposes it.
5. **Fixed-team UI prominent; split project state.** `TeamsSection` copy and
   separate `useRolePacksProject` state (`AgentsView.tsx:79-80, 362-393`).
6. **No setup stage model.** Sections render in markup order; Check draft
   precedes authoring; raw journal/relay text and hashes lead the card; the
   Roles page gives no sign a draft exists; the displayed TS brief differs from
   the native brief actually sent.
7. **Setup/lead roles make existing instructions binding.** "Follow the
   assigned project's instructions" with no reconciliation guidance.

## Shared IPC contract (native lane implements, UI lanes consume)

Frontend types and wrappers live in
`desktop/src/features/roles/lib/projectInstalledRoles.ts` (created by the
finalizer; UI lanes import, do not redefine).

1. `project_team_list_installed_roles({ expectedRelayUrl })` → read-only.
   Scans this owner's setup journals bound to that relay and returns one entry
   per journal whose installation recorded roles:
   `{ projectRef, setupId, publicationId, teamId, source: {repoRef, sha,
   packPath} | null, leadChannelId: string | null, roles: [{ role,
   agentPubkey, packRef }] }`. Never creates, repairs or publishes anything.
   A malformed journal is skipped (logged), not fatal.
2. `project_team_setup_get_activation` — `lead` gains `leadPubkey: string |
   null` (the journal's reserved lead identity).
3. `project_team_setup_get_brief({ projectRef, expectedRelayUrl, setupId })` →
   `{ text: string }`: the exact native brief this setup writes/sent. Read-only.

## Lanes and exclusive ownership

| Lane | Owns | Delivers |
| --- | --- | --- |
| N native | `desktop/src-tauri/**` | IPC 1–3 with tests; lead first message gains reconciliation guidance (§7) |
| P packs | `personas/roles/**`, `crates/buzz-persona/tests/**` | §7 role text, generic (no project or person names) |
| A agents | `desktop/src/features/agents/**` | §2 and §5 |
| C sessions | `desktop/src/features/coding-sessions/**` | §1 composer access; §3 lead picker; launcher wording |
| R roles | `desktop/src/features/roles/**` (except the finalizer's contract file), `desktop/src/testing/e2eBridge*.ts` handlers for IPC 1–3 | §6; §3/§4 installed-roles row with names, pubkeys and inline rename |

Cross-lane requests go through the finalizer, never by editing another lane's
files.

## Acceptance

- The founder of a session in a project transport can steer after launch,
  navigation and reload when the relay would admit them (explicit member, or
  project owner/collaborator on a transport). A project viewer, a per-session
  viewer invitee, or a non-member of an ordinary channel stays read-only, and
  the copy says which. An unresolved roster says "checking access", never
  "join this channel".
- Tank Loop's installed agents appear under the Tank Loop filter before any
  seat, stopped or running, with role and project shown, and can be renamed
  in place.
- With a project selected, "Who leads" shows that project's installed lead
  first (name, role, short pubkey), defaults to it when exactly one exists,
  and separates other agents on this computer.
- Renaming keeps the pubkey and role; the new name shows in the directory,
  picker and setup card.
- Agents screen: one selected project drives the directory and role-pack
  selector; saved teams remain available but no longer lead the page.
- Setup shows one current stage and one next action (author → check & save →
  publish → install → start lead), keeps retries identical, and puts hashes,
  refs and raw messages under details with plain-language summaries.
- Role text flags historical agent names, budgets and staffing assumptions for
  reconciliation while preserving product, security and review requirements.
