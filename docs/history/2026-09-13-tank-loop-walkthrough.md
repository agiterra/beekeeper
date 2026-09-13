# 2026-09-13 — Tank Loop setup walkthrough: causes and fixes

**Status:** candidate on `work/tank-loop-walkthrough-opus` from `main`
`5463b08f5`: code commit `1622e1210` plus this report's docs commit; not
pushed, not installed. Brian's live Tank Loop setup, Loom,
the running "Project State Planning" session, published packs and saved teams
were read, never changed. Binding contract:
[`../TANK_LOOP_WALKTHROUGH_IMPL.md`](../TANK_LOOP_WALKTHROUGH_IMPL.md).

## Live evidence (read-only, 2026-09-13)

Collected with the installed `bee 0.1.0 (b2e4c35c)` as Brian's identity
against hive, and from the installed app's local state. No event published.

- The session record (provider state, session `c9463aca…`, command
  `csl-7bc9063f…`) is founded by `3d3b7169…` (Brian), actor `1c47d440…`
  (Loom, role `lead`), project `30621:6cbdf445…:tank-loop`, channel
  `6620be79-0ddf-40e1-bff4-64e9f3001b03`, with
  `packRef = tank-loop-packs-43aa15fa1848@f0132d1…/personas/roles/lead`.
- Channel `6620be79` ("Tank Loop sessions") has kind:39000 tags `private`,
  `hidden`, `closed`, `t=transport`, `project=30621:6cbdf445…:tank-loop`. Its
  kind:9007 was signed by `6cbdf445…` (Andy) at 2026-08-31. Its membership
  events add only bots: Andy's `7464daa5…` (twice), then from Brian's key
  `83dcdb52…` and the provider `1958c6c4…` at 21:13 UTC and the provider and
  Loom at 21:51 UTC. No event ever adds Brian; no remove/leave event exists.
  `bee channels members` lists owner Andy plus four bots.
- The project roster lists both Andy and Brian as `owner`.
- A second project transport, "tank-loop sessions" `271feec9…`, was created by
  Brian at 21:37 UTC with Brian as owner and no bots. The setup publication
  journal records it as the lead channel with lead status `ready` and no
  session: the setup's Start project lead was never used.
- Setup authoring (`launch.json`, `authoring.json`) used `6620be79`. The lead
  session was then started from the ordinary new-session launcher with Loom
  chosen as lead, which resolved the project's existing transport — again
  `6620be79`.
- Installation journal: six roles installed at adopted commit `f0132d1…`
  (lead Loom `1c47d440…`, builder, verifier, runner, designer, project-setup);
  the live session's packRef is that same commit.
- Loom's installed system prompt repeats Tank Loop's historical token-budget
  guidance ("orchestrated fan-out costs roughly three times…"), consistent
  with issue 7.

## Confirmed causes and hypotheses

Confirmed means shown by the live evidence above or by code with a test.
Hypothesis means consistent with the evidence but not demonstrated.

1. **Founder shown view-only — confirmed.** Brian is a Tank Loop project
   owner working in a project transport channel with no membership row. The
   relay admits coding-session writes there from the project creator and
   roster owners/collaborators without membership
   (`crates/buzz-relay/src/handlers/ingest.rs:946-958`); the workspace
   composer read only `channel.isMember`
   (`CodingSessionWorkspace.tsx:204` on `5463b08f5`). The relay would have
   accepted his turns; the client refused to offer the control. Neither
   authorization nor subscriptions were at fault, and nothing was removed:
   there is no leave or remove event.
   - *Why that channel — confirmed:* the ordinary launcher and setup
     authoring both resolve the project's first existing transport
     (`resolveProjectSessionsChannel` rule 0), Andy's `6620be79`. The setup's
     own lead channel `271feec9` was never used because the lead was started
     from the ordinary launcher, not from setup.
   - *Hypothesis, not fixed:* duplicate project session channels will keep
     accumulating while setup (`ensure_lead_channel`) and the launcher use
     different selection rules.
   - *Adjacent, not fixed:* five other surfaces admit "member or any
     transport" (`channel.isMember || isSessionTransportChannel(channel)`),
     which is looser than the relay for project viewers. They gate hiring,
     capacity and progress reads, not steering.
2. **Installed agents missing under the project filter — confirmed.** The
   filter matched only seat participation (`agentDirectoryModel.ts:248`); the
   only durable installation fact is the setup publication journal, which no
   frontend read listed.
3. **Ambiguous lead picker — confirmed.** Options were every managed agent
   with any role, labelled `name · role`, with no project scoping
   (`useCodingSessionFoundedSetup.ts:242-261`). *Pack delivery verified:* the
   live session's packRef equals the adopted installation commit `f0132d1`.
4. **Rename hard to reach — confirmed.** `update_managed_agent {pubkey,
   name}` already renamed in place; no project or setup surface exposed it.
   *Found in passing, not fixed:* project installation publishes no kind:0
   profile, and a re-publication from a fresh setup journal would mint a new
   `team_id`, so identities matched by team could be duplicated.
5. **Fixed-team UI prominent; split project state — confirmed.**
   `TeamsSection` led the page and the role-pack selector held its own project
   state with a "newest checkout" fallback (`useRolePacksProject.ts`,
   `rolePacksProject.ts:94-107`).
6. **Setup progress unclear — confirmed.** No stage model; markup order put
   Check draft before authoring; the page gave no sign a draft existed; the
   displayed TypeScript brief was not the native brief the agent receives;
   `projectTeamSetupError` dropped the native error code.
7. **Setup and lead roles made existing instructions binding — confirmed in
   text.** "Follow the assigned project's instructions" with no reconciliation
   guidance, in every shipped role. Loom's installed prompt repeats Tank
   Loop's historical budget guidance. Tank Loop's already-published pack is
   unchanged by this work; the new guidance reaches new setups only.
8. **Found in review — pre-existing, partially mitigated.**
   `project_team_setup_get_publication` and `get_publication_options`
   reconcile and save the publication journal without the publication lock
   (`project_team_setup_publication.rs:935-955, 846-852`). An opened setup
   dialog can therefore rewrite an Adopted journal to SourceUnknown when the
   pack-source read comes back empty. This branch adds a no-write peek and
   keeps every mount path on read-only commands, but the dialog's existing
   reads still reconcile unlocked. Owed its own fix.

## What changed

- **Composer access** mirrors the relay: explicit member, or on a transport a
  creator of an existing project or a roster owner/collaborator. Viewers,
  per-session invitees and ordinary non-members stay read-only with distinct
  copy; an unknown roster or a truncated project list says access is being
  checked or could not be confirmed, never "join this channel". Close,
  reopen, add provider, reconnect, stop and interrupt use the same value;
  founder/operator authority is unchanged.
- **Agents screen**: installed-for-project is its own fact; the project filter
  matches seated or installed agents; rows say "Installed for Tank Loop ·
  lead"; installed agents rename in place; one project selection drives the
  directory and role packs, always naming the target project; saved agent
  groups move into a collapsed section with every action intact.
- **Who leads**: the project's installed roles come first with name, role
  and short pubkey, others under "Other agents on this computer"; a single
  installed lead is the default in Team mode; an explicit choice is never
  overwritten; Solo is unchanged. Launcher copy says the lead brings in
  workers as the task needs.
- **Setup**: one stage and next action (author → check and save → publish →
  install → start lead) with uncertain and blocked states that never read as
  done; authoring first; technical identifiers under details; the native
  brief shown; plain-language failures keeping the raw detail; a Continue
  button and "last recorded" status on the Roles page, from read-only
  commands only; installed roles with names, pubkeys and inline rename.
- **Native** (read-only): `project_team_list_installed_roles`,
  `project_team_setup_get_brief`, `project_team_setup_peek_publication`, and
  `leadPubkey` on the lead projection. The lead's first message now asks it
  to reconcile named agents and budgets and to inspect before asking;
  already-reserved leads resend their stored event unchanged.
- **Role packs**: all eight shipped roles treat named historical agents,
  reviewers, budgets and staffing as requirements to confirm; setup
  reconciles instead of copying, respects generated-instruction workflows,
  and never drops independent review.

## Review

One bounded adversarial reviewer attacked authorization, draft/data loss and
saved-configuration preservation. Its blocker was real and fixed before any
commit: the new Roles-page summary called `project_team_setup_get_publication`
on mount, and that command reconciles and saves the journal without the
publication lock, so a page visit could rewrite an Adopted journal. The
summary now uses a new no-write `project_team_setup_peek_publication` and
describes status as last recorded; a component test fails if any mount path
calls a reconciling read. Minor findings fixed: a project creator is a writer
only when the project still exists and its address is canonical; a truncated
project list reports access as unconfirmed; failure copy no longer blames the
relay for local failures; the role-pack target project is always named.
Attacked and held: pending roster tiers (none exist), pubkey case, spoofed
rosters, non-transport channels with a project tag, every other membership
consumer, native read-only listing/brief, rename sending only `{pubkey,
name}`, retry identity, and every saved-team action.

## Checks

Worktree `review-tank-loop-walkthrough-opus`; logs under
`../review-2026-09-13-tank-loop-walkthrough/` (this machine only). No full
`just ci` or `just smoke`, per the brief.

| Check | Result |
| --- | --- |
| Native focused `cargo test --manifest-path desktop/src-tauri/Cargo.toml project_team` | 82 passed, 0 failed |
| Native clippy `--all-targets -D warnings`, `cargo fmt --check` (tauri and workspace) | clean |
| `cargo test -p buzz-persona` | lib 167, e2e_env_flow 5, integration 13, pack_rules 8; 0 failed |
| Desktop unit suite (`pnpm test`) on the integrated tree | 9433 passed, 0 failed |
| `just desktop-check` (biome, px-text, pubkey truncation, e2e registration), `tsc --noEmit`, file-size ratchet, current-state gate | clean; biome's 3 warnings predate this branch |
| Affected browser specs (setup, crew front door, role packs, agents, team snapshot, founded setup, edit agent, workspace reuse, native steer) | 53 passed and 3 failed on first run; the three were expectations for intended changes, fixed and rerun: 20/20 |
| New walkthrough spec `project-roles-walkthrough.spec.ts` plus founded setup, final build | 10 passed |

## Remaining live checks (installed app, not done)

Nothing was installed. After Astra integrates and installs:

1. Reopen "Project State Planning" as Brian: the composer should read "Can
   control" and accept a turn in channel `6620be79` without joining it. Open
   the same session as a Tank Loop viewer if one exists: read-only, with the
   project-role sentence.
2. Agents → project filter Tank Loop: all six installed Tank Loop identities
   listed, stopped ones included, "Installed for Tank Loop · <role>"; rename
   one and confirm the same pubkey keeps the new name in the picker.
3. New Tank Loop session in Team mode: Loom preselected under "Tank Loop
   project roles"; after start, the session's 44223 `packRef.sha` still equals
   the adopted commit.
4. Roles page: "Continue project role setup" with the last recorded status;
   the setup dialog's stage and next action; confirm Tank Loop's
   `publication.json` status is unchanged by merely visiting Roles.
5. A new project setup reports reconciliation items for named historical
   agents and budgets instead of treating them as requirements.

## Limits

- Tank Loop's already-published and installed packs keep their old text,
  including Loom's historical budget guidance. This branch does not publish a
  replacement; changing Tank Loop's roles is a project decision.
- The setup dialog's existing publication reads still reconcile unlocked
  (finding 8).
- Duplicate project session channels can still accumulate between setup and
  the launcher (finding 1, hypothesis).
- A per-session viewer invitee and an unresolved-roster composer state are
  covered by unit tests, not by the browser spec.
