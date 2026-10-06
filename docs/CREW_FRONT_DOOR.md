# The front door — role packs become a crew you can seat

_Written 2026-08-27 by the designer seat, from the tree at `crew/front-door`
(`b750941a`). It is a spec plus three complete lane briefs. The lead dispatches
the briefs verbatim; a builder treats its own brief as law and reports back
rather than improvising (`docs/CREW_SESSIONS_PLAN.md` §1)._

## Why this exists

Six slices shipped a wire contract, a CLI and a provider. Almost none of it
has a front door:

- Role packs exist in-repo (`personas/roles/**`, seven of them) and a seat can
  carry one, but **nothing on this computer turns a pack into an agent** —
  `resolve_seat_pack` (`desktop/src-tauri/src/managed_agents/actor_seats.rs:115`)
  needs `persona_team_dir`/`persona_name_in_team` or a team's `source_dir`, and
  `AgentDefinition::into_agent_record` (`types.rs:131`) writes `None` for both.
  Every seat launched tonight therefore staged **without** its pack.
- `TeamRecord.crew` exists (`managed_agents/types/team.rs:24`) and the Crew tab
  reads it off raw `list_teams`
  (`desktop/src/features/coding-sessions/lib/codingSessionCrewTeams.ts:44`),
  but every writer in the tree sets `crew: None` (`teams.rs:61`,
  `commands/teams.rs:335,512`, `team_snapshot.rs:242`,
  `commands/team_snapshot.rs:173`). **No crew can be authored from the UI at
  all.**
- A seat can only be created while *founding* a session. `AddCodingSessionProviderDialog`
  — the join path — has no seat field, so an agent cannot be added to a session
  that is already running.
- The lead wrote a raw authenticated REQ by hand three times in `/tmp` tonight
  because `bee` has no `events query`. And `bee sessions status` lists
  executions with no founder, which is how a probe meant for a quiet session
  landed in Andy's (SESSION_STATE item 73).

Three lanes close those four holes. Everything else waits.

## Rules from the lead (locked)

- **identity = who signs. role = what the seat is for. pack = what the role knows.**
- An **actor** carries a **home role** and the pack behind it. A **seat**
  carries the role **on the wire**, unchanged (`actor` + `role` on the 44221,
  `agentRef` + `role` on the 44223). A seat may be given a role that is not the
  actor's home role — and when it is, the UI says so, because the pack that
  gets staged is still the **home** role's pack.
- Key custody never crosses the wire (D6) and packs are host-local. A crew
  exported to another computer arrives with no packs; its seats are seated with
  no role skills, and every screen that shows them says exactly that.

## The cross-lane contract

Every name below is fixed. Lane A owns the Rust→TypeScript boundary in one
file set so that both sides of a field land in one commit; lane B consumes the
new fields through a **structural** type of its own so its branch compiles
before lane A's lands.

### Rust — `ManagedAgentRecord` (`desktop/src-tauri/src/managed_agents/types.rs`)

Plain `Serialize`/`Deserialize`, no `rename_all`: the JSON key is the field
name.

```rust
/// The role this agent *is*, taken from its pack persona's `role:`
/// frontmatter at install time. `None` for every agent that predates the
/// crew-role installer and for any persona that declares no role — never
/// guessed from a name.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub home_role: Option<String>,
```

### Rust — `ManagedAgentSummary` (same file, plain `Serialize`)

```rust
/// Mirror of `ManagedAgentRecord.home_role`.
pub home_role: Option<String>,
/// Whether this computer can stage a role pack for this agent — exactly
/// `resolve_seat_pack(record, &teams).is_some()`. `false` means a seat on
/// this agent runs on its persona prompt alone.
pub has_role_pack: bool,
```

### Rust — the crew block (unchanged, restated so both lanes read one shape)

`TeamCrew` / `TeamCrewSeat` in
`desktop/src-tauri/src/managed_agents/team_events.rs:27-57`,
`#[serde(rename_all = "camelCase")]`:

```json
{
  "primary": "<personaId of the seat that gets the first turn>",
  "seats": [
    { "personaId": "<definition id>", "role": "lead" },
    { "personaId": "<definition id>", "role": "architect" },
    { "personaId": "<definition id>", "role": "builder" },
    { "personaId": "<definition id>", "role": "verifier" },
    { "personaId": "<definition id>", "role": "runner" }
  ]
}
```

`driver`, `model`, `vendor` stay optional and the installer writes none of
them: a seat with no model falls back to the agent's, then to the dialog's
(`resolveCodingSessionCrewSeats`), which is the behaviour the crew launch
already checks against the runtime.

### Rust — new Tauri commands (lane A)

```rust
#[tauri::command]
pub async fn install_crew_role_packs(
    app: AppHandle,
    state: State<'_, AppState>,
    directory: String,
) -> Result<InstallCrewRolePacksResponse, String>;

#[tauri::command]
pub async fn pick_crew_role_packs_directory(app: AppHandle) -> Result<Option<String>, String>;
```

`pick_crew_role_packs_directory` is `pick_coding_session_workdir`
(`desktop/src-tauri/src/coding_sessions/workdir_store.rs:392`) with the title
`"Choose a folder of role packs"`; `tauri-plugin-dialog` is already a
dependency and already granted.

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCrewRolePacksResponse {
    pub team_id: String,
    pub team_name: String,
    pub installed: Vec<InstalledCrewRole>,
    pub skipped: Vec<SkippedCrewRolePack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledCrewRole {
    pub persona_id: String,
    pub persona_name: String,
    pub role: String,
    pub agent_pubkey: String,
    pub agent_name: String,
    pub pack_dir: String,
    /// `true` when an agent already installed from this pack was refreshed
    /// rather than minted.
    pub refreshed: bool,
    /// `true` when this role is in the crew's default seat roster.
    pub seated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedCrewRolePack {
    pub path: String,
    pub reason: String,
}
```

New structs are camelCase on the wire; the legacy `ManagedAgentSummary` /
`TeamRecord` stay snake_case and are mapped in `tauri.ts` / `tauriTeams.ts` as
they are today. Do not "tidy" either direction.

### Rust — snapshot (lane A)

```rust
// TeamSnapshotMeta (camelCase)
#[serde(default, skip_serializing_if = "Option::is_none")]
pub crew: Option<TeamSnapshotCrew>,

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshotCrew {
    /// `AgentSnapshotDefinition.name` of the seat that takes the first turn.
    pub primary_member_name: String,
    pub seats: Vec<TeamSnapshotCrewSeat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshotCrewSeat {
    pub member_name: String,
    pub role: String,
}

// AgentSnapshotDefinition (camelCase)
#[serde(default, skip_serializing_if = "Option::is_none")]
pub home_role: Option<String>,
```

A snapshot's crew is keyed by **member name**, never by `personaId`: import
mints fresh definition ids, so ids in a snapshot are meaningless on the
importing computer. `pack_dir` is host-local and is never exported — the
existing sentinel tests in `agent_snapshot_tests.rs:501-517` must keep passing.

### TypeScript — types (lane A owns `desktop/src/shared/api/types.ts`)

```ts
// ManagedAgent
/** The role this agent is, from its pack persona. `null` when it has none. */
homeRole: string | null;
/** Whether this computer can stage a role pack for it. */
hasRolePack: boolean;

// AgentTeam
/** Crew composition, or `null` for an ordinary team. */
crew: AgentTeamCrew | null;

export type AgentTeamCrewSeat = {
  personaId: string;
  role: string;
  driver?: string | null;
  model?: string | null;
  vendor?: string | null;
};
export type AgentTeamCrew = { primary: string; seats: AgentTeamCrewSeat[] };
```

Raw shapes in `desktop/src/shared/api/tauri.ts` (`RawManagedAgent.home_role?:
string | null`, `RawManagedAgent.has_role_pack?: boolean`) and
`tauriTeams.ts` (`RawTeam.crew?: RawTeamCrew | null`), mapped in
`fromRawManagedAgent` / `fromRawTeam` with `?? null` / `?? false` so an older
backend degrades to "no home role, no pack" rather than crashing.

### TypeScript — lane B's structural seat type

`desktop/src/features/coding-sessions/lib/codingSessionSeatAgent.ts` (new, lane
B owns it):

```ts
/** As much of a managed agent as a seat field needs. Structural on purpose:
 *  `ManagedAgent` satisfies it today, and gains the optional fields when lane
 *  A lands, with no coupling between the two branches. */
export type CodingSessionSeatAgent = {
  pubkey: string;
  name: string;
  status?: string;
  homeRole?: string | null;
  hasRolePack?: boolean;
};
```

`NewCodingSessionAgentSeatField` takes `readonly CodingSessionSeatAgent[]`
instead of `readonly ManagedAgent[]`. Until lane A lands, `homeRole` and
`hasRolePack` are `undefined` and the field behaves exactly as it does today —
no default role, no mismatch line, no pack line. That is the honest
degradation: absent is not `false`.

**Deviation from the batch brief, on purpose:** the batch gave `types.ts` to
lane B. It moves to lane A because two branches cannot both compile against a
field declared in a file only one of them owns; lane B's structural type
removes the need entirely.

### TypeScript — seat custody staging result (lane B)

`publishSeatedCodingSessionCreate` and `publishSeatedCodingSessionResume`
(`codingSessionSeatedCreate.ts`) gain one optional input, keeping their return
type `T`:

```ts
/** Called with what staging actually put on disk, before the publish. */
onSeatStaged?: (staged: { packStaged: boolean }) => void;
```

Today `publishWithStagedSeat` awaits `deps.stageSeat(...)` and throws the
result away (`codingSessionSeatedCreate.ts:63`), which is why the one-session
path cannot say what the Crew tab says.

### Copy strings (verbatim — a lane that invents copy invents a comfortable version)

Lane A:

- Button, on the "New team" card: `Install crew roles…`
- Dialog title: `Install crew roles`
- Dialog body: `Pick a folder of role packs. Every pack whose persona declares a role becomes one agent on this computer — carrying that role and that pack — and they all join one team you can launch as a crew.`
- Submit button: `Choose folder…`
- Success toast: `Installed {n} crew roles into “{team}”: {roles}.`
- Refresh note (per row, in the result list): `already installed from this pack — role and pack link refreshed`
- Skipped row: `{path}: no persona in this pack declares a role, so it was skipped.`
- Nothing found: `No role packs in that folder. A role pack is a directory holding .plugin/plugin.json whose persona declares “role:” in its frontmatter.`
- Unreadable folder: `That folder could not be read: {error}`
- Roster note in the dialog and in the result: `Seated by default: lead, architect, builder, verifier, runner. The poker and designer packs are installed as agents but not seated — the poker drives the built app, and the designer works before a crew launches.`
- Team card badge: `Crew · {n} seats`
- Agent row badge: `Home role: {Role}`
- Agent row, no pack: `No role pack on this computer`

Lane B:

- Role default note, when the role equals the home role: `Its home role.`
- Mismatch: `{Agent} is a {homeRole} — seating it as {role}; it will carry the {homeRole} pack.`
- No pack, in the seat field before submit: `{Agent} has no role pack on this computer, so this seat carries no role skills and runs on its persona prompt alone.`
- Pending screen seat line, seated: `Seated: {Agent} · {Role}`
- Pending screen seat line, seated with no pack staged: `Seated: {Agent} · {Role} — seated with no role skills: this computer has no role pack behind this persona.`
- Session header seat chip: `{Agent} · {Role}`
- Add-provider dialog, seat field header: `Seat an agent (optional)` (the founding field's own label, reused)

Lane C:

- `bee events query` help: `Run a raw authenticated REQ against the relay. --kinds is required; the relay's p-gate refuses a filter without it.`
- Missing kinds: `--kinds is required: a filter with no kinds is refused by the relay with 403.`
- Founder unknown: JSON `null`, never a guess; compact prints `founder: null`.

---
## LANE A — role packs become actors with a home role, and a crew can be authored

```
LANE A — install the repo's role packs as agents with a home role, under one team that is a crew.
Tier: 2 — it touches durable managed-agent/team state, key minting, and the seat-pack resolution
      the provider consumes (custody-adjacent).
Branch: crew/lane-a, worktree from crew/front-door @ b750941a.

Owns (exclusive):
  desktop/src-tauri/src/managed_agents/**            (types.rs, types/team.rs, teams.rs,
                                                      actor_seats.rs, team_snapshot.rs,
                                                      agent_snapshot.rs, storage.rs, and a new
                                                      crew_roles.rs; plus their tests)
  desktop/src-tauri/src/commands/{teams.rs,agents.rs,team_snapshot.rs}
  desktop/src-tauri/src/commands/crew_roles.rs       (new)
  desktop/src-tauri/src/handlers.rs                  (registration lines ONLY — no other lane
                                                      touches this file in this batch)
  desktop/src/features/agents/**
  desktop/src/features/settings/ui/AgentsSettingsPanel.tsx
  desktop/src/shared/api/{types.ts,tauri.ts,tauriTeams.ts}
  and the tests for all of the above.
Must not touch anything else; if a correct change needs a file outside this list, STOP and report
it with the exact path and why. (Exception: an exhaustive struct literal in another crate's tests
that stops compiling because you added a field — add the field mechanically and report it.)

Problem, with evidence:
  - `AgentDefinition::into_agent_record` writes `persona_team_dir: None` and
    `persona_name_in_team: None` (desktop/src-tauri/src/managed_agents/types.rs:131-132), so
    `resolve_seat_pack` (actor_seats.rs:115-152) resolves nothing for every agent created today and
    every seat stages without its pack. SESSION_STATE 2026-08-27 records seats briefed without their
    pack from the live run.
  - Every writer of `TeamRecord.crew` sets `None`: managed_agents/teams.rs:61,
    commands/teams.rs:335, commands/teams.rs:512, managed_agents/team_snapshot.rs:242,
    commands/team_snapshot.rs:173. The Crew tab reads crews off raw `list_teams`
    (codingSessionCrewTeams.ts:44) and therefore lists nothing, ever.
  - There is no "role" on an agent at all: `ManagedAgentRecord` (types.rs:215-410) has no field for
    it, so the desktop cannot default a seat's role or disclose a mismatch.

Design (LOCKED):
 1. `ManagedAgentRecord.home_role: Option<String>` and the two mirrors on `ManagedAgentSummary`
    (`home_role`, `has_role_pack`) exactly as the contract section spells them. `home_role` is only
    ever written from a pack persona's `role:` frontmatter
    (`beekeeper_persona_pkg::resolve::resolve_persona_by_name(...).role`) — never inferred from a name,
    a slug, or a team.
 2. New command `install_crew_role_packs(directory)`:
    a. Refuse a path that is not a readable directory, with the "unreadable folder" copy.
    b. Scan its immediate children (depth 1, no recursion). A child is a role pack when it holds
       `.plugin/plugin.json` and `resolve_pack` yields at least one persona whose `role` is `Some`.
       Anything else lands in `skipped` with the "no persona declares a role" reason — never
       silently dropped.
    c. For each role pack, in `role` order (lead, architect, builder, verifier, runner, poker,
       designer, then any unknown role alphabetically), mint one definition + one managed agent
       whose `home_role` is the persona's role, `persona_team_dir` is that pack's absolute
       directory, `persona_name_in_team` is the persona's name, and `source_team`/
       `source_team_persona_slug` are set to the team id / persona name so the provenance arm of
       `resolve_seat_pack` also resolves. Reuse the existing create path (`create_managed_agent`'s
       helpers) — do NOT write a second key-minting path.
    d. Idempotent: an agent already carrying this `persona_team_dir` + `persona_name_in_team` is
       refreshed (home_role, pack link, display name) and reported `refreshed: true`, never
       duplicated. Re-running the installer twice must produce the same agents and the same crew.
    e. All of them join one team named `Crew roles` (deduped by that name + a `home_role`-carrying
       membership, so a second run updates it rather than making `Crew roles (2)`).
 3. **The team's `source_dir` stays `None`.** `delete_team_with_cascade` does
    `fs::remove_dir_all(source_dir)` (managed_agents/teams.rs:262-289): pointing a team at the
    repo's `personas/roles` would make "Delete team" delete the operator's checkout. The pack link
    lives on each agent instead. Write a test that pins this.
 4. The team gets a crew block: `primary` = the lead seat's `personaId`; `seats` in launch order
    `lead, architect, builder, verifier, runner`. Poker and designer packs are installed as agents
    but are **not** seated: a poker drives the built app (screenshots, e2e) which a seated
    coding-session execution cannot do, and a designer works at brief time, before a crew exists.
    Say so in the dialog with the roster copy. A pack whose role is missing from the roster is
    installed and left unseated; a roster role with no installed pack is dropped from `seats` and
    reported — never seated with a persona id that does not exist.
 5. Snapshot: `TeamSnapshotMeta.crew` keyed by member NAME, `AgentSnapshotDefinition.home_role`,
    both `#[serde(default)]`. On import, remap each seat's `memberName` to the freshly minted
    definition id; if any seat or the primary cannot be remapped, import the team **with no crew**
    and surface `This snapshot's crew could not be matched to its members, so it was imported as an
    ordinary team.` in the import preview. Host-local paths are still never exported — the sentinel
    assertions at agent_snapshot_tests.rs:501-517 must stay green.
 6. `list_managed_agents` computes `has_role_pack` with one `load_teams` per call, not one per
    agent.

Contract changes: the Rust and TypeScript sections of docs/CREW_FRONT_DOOR.md, verbatim. Update
  docs/CREW_ROLES.md § Materialization ("Managed agents currently materialize nothing") in the same
  commit — it becomes false the moment this lands.

Surfaces:
  desktop: desktop/src/features/agents/ui/TeamsSection.tsx (`NewTeamCard`, TeamsSection.tsx:191-217)
    entry point: the "New team" card's dropdown gains a third item, `Install crew roles…`, between
      `Create team` and `Import`, `data-testid="install-crew-roles"`.
    fields/states: a new `InstallCrewRolesDialog.tsx` in the same folder.
      - idle: the dialog body copy, a read-only folder path field (empty), `Choose folder…`
        (calls `pick_crew_role_packs_directory`), the roster note, and a disabled `Install`.
      - chosen: path shown, `Install` enabled.
      - installing: `Install` disabled + spinner, dialog not dismissible.
      - done: a result list — one row per installed role (`{Role} — {agentName}`, plus
        `already installed from this pack — role and pack link refreshed` when `refreshed`), then
        one row per skipped path with its reason, then the roster note. Close is the only action.
      - nothing found: the "No role packs in that folder." copy, `Install` re-enabled.
      - failed: the "That folder could not be read: {error}" copy.
      Team card gains `Crew · {n} seats` when `team.crew` is non-null
      (TeamsSection.tsx, on `TeamIdentityCard`). Agent rows
      (desktop/src/features/agents/ui/ManagedAgentRow.tsx) gain `Home role: {Role}` when
      `homeRole` is non-null, and `No role pack on this computer` when `hasRolePack` is false and
      `homeRole` is non-null — a plain agent with neither shows neither badge.
    failure copy: the six lane-A strings in the contract section, verbatim.
    disclosure: `No role pack on this computer` is the whole point of `has_role_pack`. An agent
      that carries a home role but no pack must never render as if it carried the role's craft.
  mobile: no surface, by decision — installing packs and minting keys is desktop-host-local
    (custody lives in the desktop keyring). *Amended 2026-09-07 (Andy):* the Flutter app is no
    longer read-only — it steers existing sessions and types into shared terminals (SESSION_STATE
    "Found 2026-09-07") — but it still installs nothing and mints nothing.
  web: no surface, by decision — same reason; the web client holds no keys. (needs Brian's sign-off)
  CLI: no surface, by decision — `bee` cannot reach the desktop's managed-agent store or keyring,
    and a second minting path is exactly the kind of divergence that produces an agent the provider
    cannot resolve. (needs Brian's sign-off)

Tests you must add (red first, then green):
  Rust (cargo test --manifest-path desktop/src-tauri/Cargo.toml):
    - `home_role_round_trips_and_is_absent_on_old_records` — a stored record with no `home_role`
      key deserializes to `None`, and serializing `None` omits the key.
    - `install_scans_only_immediate_children_and_skips_packs_with_no_role`
    - `install_is_idempotent_and_refreshes_rather_than_duplicating`
    - `installed_agents_resolve_a_seat_pack` — `resolve_seat_pack` returns `Some` for every agent
      the installer minted (this is the bug: it returns `None` today).
    - `the_crew_roles_team_has_no_source_dir_so_delete_cannot_remove_the_packs`
    - `the_installed_team_carries_a_crew_in_launch_order_with_the_lead_primary`
    - `a_roster_role_with_no_pack_is_dropped_from_the_seats_not_seated_with_a_missing_persona`
    - `a_team_snapshot_round_trips_its_crew_by_member_name`
    - `a_snapshot_crew_whose_member_is_missing_imports_as_an_ordinary_team`
    - `a_snapshot_still_carries_no_host_local_pack_path` (extends the existing sentinel test)
  TypeScript (cd desktop && pnpm test):
    - `installCrewRolesDialog.test.mjs` — result list renders installed, refreshed and skipped rows
      and the roster note; the "nothing found" state renders its copy.
    - `tauri.test.mjs` additions — `fromRawManagedAgent` maps `home_role`/`has_role_pack` and
      defaults them to `null`/`false` when the backend omits them.

Acceptance (report the exact counts):
  cargo test --manifest-path desktop/src-tauri/Cargo.toml   → 0 failed
  cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings → clean
  cd desktop && pnpm test && pnpm typecheck && pnpm check:px-text → 0 failed / clean
  Live, on this computer: run the installer against the repo's `personas/roles`, then confirm
  (a) `Crew roles` appears in the Crew tab of the new-session dialog with five seats, and
  (b) a seat launched from it stages WITH a pack (the Crew tab no longer shows the
      "carry no role skills" line for it).

Out of scope (named temptations): a crew editor (adding/removing/reordering seats by hand); a
  second builder seat (clone the builder agent, then edit the crew — no cloning UI in this lane);
  making `home_role` editable in the agent dialog (it comes from the pack; an editable copy is a
  label that can diverge from the pack actually staged); publishing `home_role` on kind:30175;
  recursing into nested pack directories.

Report format: docs/CREW_SESSIONS_PLAN.md §1.2.
```

---
## LANE B — seats in the session UI

```
LANE B — an agent can be seated on a running session, its role defaults to its home role, and every
         screen says when a seat has no pack behind it.
Tier: 2 — it touches the seated-create path (custody staging + membership before a signed publish).
Branch: crew/lane-b, worktree from crew/front-door @ b750941a.

Owns (exclusive):
  desktop/src/features/coding-sessions/**   (ui + lib + hooks, EXCEPT nothing lane A lists —
                                             lane A touches no file under this directory)
  and the tests for all of the above.
Must not touch desktop/src/shared/api/** (lane A owns the Rust→TS boundary), nor anything under
desktop/src-tauri/. If a correct change needs one, STOP and report the path and why.

Problem, with evidence:
  - `AddCodingSessionProviderDialog.tsx` builds its submit through
    `buildAddCodingSessionProviderSubmit` (ui/addCodingSessionProviderModel.ts:126-165) and never
    passes a `seat`, although `useNewCodingSessionCreate.submit` already accepts one
    (ui/useNewCodingSessionCreate.ts:438-440) and does membership + custody staging + the
    grant-operator follow-up for it (:340-376, :513-522). So an agent can be seated only while
    founding a session, never on one that is running.
  - `publishWithStagedSeat` awaits `deps.stageSeat(...)` and discards its
    `{ packStaged }` (lib/codingSessionSeatedCreate.ts:63-66). The Crew tab can say
    "seated with no role skills" (lib/codingSessionCrewLaunch.ts:331) and the one-session path
    cannot say anything.
  - `NewCodingSessionAgentSeatField.tsx` has no notion of a home role: the role box starts empty
    and offers a static suggestion list (lib/codingSessionActorSeat.ts:16-23).
  - `PendingCodingSessionScreen.tsx` renders the first message and a status line and never names
    the seat, although the transaction carries `input.actor` / `input.role`.

Design (LOCKED):
 1. Seat field takes `readonly CodingSessionSeatAgent[]` (new file
    lib/codingSessionSeatAgent.ts, shape in the contract section) instead of `ManagedAgent[]`.
    `NewCodingSessionDialog` keeps passing its `useManagedAgentsQuery` array — it satisfies the
    structural type today and gains the two optional fields when lane A lands. *(2026-09-10:
    that dialog is deleted; the founded page's setup card is the caller now — see "A crew
    editor" below.)*
 2. Role default: a pure helper `defaultCodingSessionSeatRole({ agent, roleTouched, role })` in
    lib/codingSessionActorSeat.ts. Selecting an agent while the role box is untouched sets the role
    to `agent.homeRole ?? ""`. Typing in the role box marks it touched; clearing the agent clears
    both. An agent with no `homeRole` sets nothing (today's behaviour).
 3. Mismatch disclosure: a pure `codingSessionSeatRoleNotice({ agent, role })` returning
    `{ tone: "muted" | "warn", message }` or `null`:
      - `role === agent.homeRole` → `Its home role.`
      - `agent.homeRole` present and different → the mismatch string, verbatim.
      - `agent.homeRole` absent/undefined → `null` (never "unknown role" — absence is not a claim).
 4. Pack disclosure before submit: when `agent.hasRolePack === false`, render the "no role pack"
    string under the role box. `undefined` renders nothing — a lane-A-less build must not accuse an
    agent of lacking a pack it was never asked about.
 5. `publishSeatedCodingSessionCreate` / `...Resume` gain `onSeatStaged` (contract section).
    `useNewCodingSessionCreate` stores it as `seatPackStaged: boolean | null` (null = never staged
    in this process, e.g. a rehydrated durable create) and returns it plus
    `seat: { actor, role } | null` derived from the transaction input.
 6. Pending screen takes `seat?: { actorLabel: string | null; role: string; packStaged: boolean | null }
    | null` and renders one line under the status line, `data-testid="pending-coding-session-seat"`:
    the "Seated:" string, extended with the no-skills clause exactly when `packStaged === false`.
    `packStaged === null` renders the short form — unknown is not "no".
 7. `CodingSessionHeader` gains `seat?: { label: string } | null`, rendered as a chip beside the
    title, `data-testid="coding-session-header-seat"`. Its label comes from
    `formatCodingSessionExecutionLabel` (lib/codingSessionLabels.ts:118) so a seated execution reads
    the same way everywhere. `CodingSessionWorkspace` passes it from the record's `agentRef`/`role`;
    the pending screen passes it from the transaction. An unseated session passes `null` and looks
    exactly as it does today.
 8. The join path: `AddCodingSessionProviderForm` owns seat state the same way
    `NewCodingSessionDialog` did (`seatActor`, `seatRole`, `resolveCodingSessionActorSeat`; the
    dialog is gone since 2026-09-10, the join form's own seat state stays), passes
    `seat`/`seatLabel` through `buildAddCodingSessionProviderSubmit` into `submit`, and blocks
    submit while `seatResolution.error !== null`. Membership, custody staging, the publish order,
    and the grant-operator follow-up are the founding path's, unchanged — do not fork them.

Contract changes: the TypeScript sections of docs/CREW_FRONT_DOOR.md (structural seat type,
  `onSeatStaged`). No wire change: a joined seated create is the same 44221 the founding path signs.

Surfaces:
  desktop: desktop/src/features/coding-sessions/ui/AddCodingSessionProviderDialog.tsx
    entry point: session header → "Add a provider" (existing `onAddProvider`), then the new
      `NewCodingSessionAgentSeatField` between the workdir field and "First message".
    fields/states: agent dropdown (`No agent — you run this session` default; every managed agent
      with its running/stopped line), role text box (appears only once an agent is chosen,
      defaulted from the home role), the role notice line, the no-pack line, the seat error line.
      States: unseated (today's dialog, byte-identical behaviour), seated+matching role,
      seated+mismatched role, seated+no pack, half-seated (refused before publish).
    failure copy: the existing `resolveCodingSessionActorSeat` strings (unchanged), plus the lane-B
      strings in the contract section.
    disclosure: mismatch and no-pack lines are mandatory — a seat that will be briefed with the
      builder pack must not present itself as a lead.
  desktop: desktop/src/features/coding-sessions/ui/PendingCodingSessionScreen.tsx
    entry point: reached automatically the moment a create is durable.
    fields/states: the new seat line, in all three states (seated / seated-no-pack / unknown).
    failure copy: unchanged from item 73's fix — this lane adds a line, it does not touch
      `newCodingSessionStatusMessage`.
    disclosure: a seated execution must never read like a human one.
  desktop: desktop/src/features/coding-sessions/ui/CodingSessionHeader.tsx — the seat chip, on both
    the pending screen and a live single-execution workspace.
  mobile: no seat surface, by decision — the phone cannot create sessions (custody is host-local),
    so it never stages a seat. *Amended 2026-09-07 (Andy):* it does now send turns, interrupt, stop,
    rename, set the goal and close existing sessions; the seat chip is read-only there.
  web: no surface, by decision — same. (needs Brian's sign-off)
  CLI: no surface, by decision — `bee sessions create --actor` is already refused on purpose
    (crew_cmds.rs:392-416): the CLI holds no host-local custody, so a seated create from `bee` would
    be answered `ACTOR_UNAVAILABLE`. Lane C surfaces the seat on READ (status/list) instead.
    (needs Brian's sign-off)

Tests you must add (red first, then green) — `cd desktop && pnpm test`:
  - `codingSessionActorSeat.test.mjs`: `defaultCodingSessionSeatRole` fills from `homeRole`, leaves
    a touched role alone, and does nothing when `homeRole` is absent.
  - `codingSessionActorSeat.test.mjs`: `codingSessionSeatRoleNotice` returns the home-role note, the
    verbatim mismatch string, and `null` when `homeRole` is absent.
  - `codingSessionSeatedCreate.test.mjs`: `onSeatStaged` receives `{ packStaged }` from `stageSeat`
    before the publish runs, and is not called when the publish is refused before staging.
  - `addCodingSessionProviderModel.test.mjs`: the built submit carries `seat` and `seatLabel`; a
    half-filled seat yields no payload.
  - `AddCodingSessionProviderDialog` test: the seat field renders, a seated submit reaches `submit`
    with the seat, and a half-seated one cannot submit.
  - `PendingCodingSessionScreen.test.mjs`: the seat line renders `Seated: …` for a seat, the
    no-skills clause exactly when `packStaged === false`, and nothing when there is no seat.
  - `CodingSessionHeader` test: the chip renders for a seated record and is absent for an unseated
    one.

Acceptance:
  cd desktop && pnpm test && pnpm typecheck && pnpm check:px-text → 0 failed / clean
  Live: with a session already running, add a provider seated as an agent, and watch the pending
  screen name the seat and the execution rail pick it up. Report what the seat line said and
  whether a pack was staged.

Out of scope: changing the wire; a role picker that hides free text; seating an agent that is not
  managed on this computer; the crew launch path (lane A owns what it launches); the composer's
  addressing UI.

Report format: docs/CREW_SESSIONS_PLAN.md §1.2.
```

---
## LANE C — the CLI

```
LANE C — `bee events query`, and a founder column on `bee sessions status` / `list`.
Tier: 0/1 — read-only decoders and one new query command; no durable state, no custody, no relay
      change. (`events query` publishes nothing.)
Branch: crew/lane-c, worktree from crew/front-door @ b750941a.

Owns (exclusive):
  crates/beekeeper-cli/**
  crates/beekeeper-sdk/**  — only if a builder is genuinely missing. It should not be: this lane reads.
                        If you touch it, say exactly which builder was missing and why.
  crates/beekeeper-cli/TESTING.md
Must not touch anything else; if a correct change needs a file outside this list, STOP and report.

Problem, with evidence:
  - There is no `Cmd::Events` (crates/beekeeper-cli/src/lib.rs:194-278). The lead hand-wrote an
    authenticated REQ three times in /tmp on 2026-08-27 because the CLI cannot run one, even though
    `BeekeeperClient::query_all` / `query_paginated` already page the relay's `/query` bridge
    (crates/beekeeper-cli/src/client.rs:683-729).
  - `bee sessions status` prints target, seat, liveness, open turn and budget
    (commands/sessions/crew_cmds.rs:493-580) and never names the founder — which is how a probe
    aimed at "a 3-day-quiet session" landed in Andy's (SESSION_STATE item 73, closing paragraph).
    `bee sessions list` (commands/sessions.rs:1154-1201) has the same gap.

Design (LOCKED):
 1. New top-level `Events(EventsCmd)` with one subcommand:
      bee events query --kinds <n>[,<n>…]        (REQUIRED)
                       [--channel <uuid> | --h <value>]   (both write the `#h` filter key)
                       [--authors <hex>[,<hex>…]]
                       [--since <rfc3339|unix>] [--until <rfc3339|unix>]
                       [--limit <n>]  [--ids <hex>[,<hex>…]]
    `--kinds` is required by clap, with the help and error strings in the contract section:
    omitting kinds trips the relay's p-gate with a 403, so refusing locally is the honest failure.
    `--limit` maps to `query_paginated(filter, limit)`; absent means `query_all`.
    Output: the raw signed events as a JSON array, exactly as the relay returned them, sorted
    newest-first by `(created_at, id)`. `--format compact` prints one reduced object per event:
    `{id, kind, pubkey, createdAt, h, summary}` where `summary` is the first 120 characters of
    `content` with newlines collapsed — never a parsed payload, so a malformed event still prints.
    Validate `--channel` as a UUID and every author/id as 64-char lowercase hex before any request.
 2. Founder, defined once in `crates/beekeeper-cli/src/commands/sessions/crew.rs` and used by both
    commands:
      - `create_signer`: the pubkey that signed the 44221 `session.create` whose `commandId` a 44224
        receipt joined to this generation's target. Fold creates from the events
        `fetch_crew_facts` already pulls (`KIND_CODING_SESSION_LIFECYCLE_COMMAND` is in
        `CREW_FACT_KINDS`, crew_cmds.rs:131-138) and join through
        `LifecycleReceipt.command_id` + `.session`.
      - `founder`: the signer of the 44226 genesis named by that create's `genesisRef`, when the
        genesis event is in the channel; otherwise `null`.
      - Resumes are NOT founders: `decode_resumes` (crew.rs:449) already exists; a resume's signer
        must never appear in either field.
      - Both fields are `Option<String>`; unknown prints `null`. Never fall back to the provider's
        signer — the provider signs every execution and would make every founder look identical.
 3. `bee sessions status`, JSON: add `"founder"` and `"createSigner"` per execution, and a
    top-level `"founders"` array of the distinct non-null founders in the channel. Compact: add
    `"founder"` (short pubkey, or `null`) after `"seat"`.
 4. `bee sessions list`, JSON: add `"founder"` and `"createSigner"` per row. Compact: add
    `"founder"`. `cmd_list` fetches only metadata + receipts today (sessions.rs:1159-1168) — extend
    its kind list with `KIND_CODING_SESSION_LIFECYCLE_COMMAND` and
    `KIND_CODING_SESSION_GENESIS`, and say in the report what that cost in events fetched.
 5. `bee sessions inbox` and `bee sessions roster` are unchanged. Inbox rows are turns, not
    executions, and the roster is an authority chain, not a founder list; adding a founder column to
    either would be a second, differently-derived answer to the same question.

Contract changes: none on the wire. Document `bee events query` and the two new columns in
  crates/beekeeper-cli/TESTING.md.

Surfaces:
  CLI: crates/beekeeper-cli/src/lib.rs (`Cmd::Events`, `EventsCmd`), dispatched from
    crates/beekeeper-cli/src/commands/events.rs (new).
    entry point: `bee events query --kinds …`; `bee --format compact events query …`.
    fields/states: the flags above; empty result prints `[]` and exits 0; a relay error exits 2; a
      bad flag exits 1 (the CLI's existing exit-code contract, AGENTS.md § Agent CLI).
    failure copy: the two lane-C strings in the contract section, verbatim.
    disclosure: `founder: null` means "this channel does not contain the record that would say" —
      it is printed as null and documented as such, never filled with the provider's key.
  desktop: no surface, by decision — the desktop already shows founder provenance in the session
    header popover (`founderDetails`, CodingSessionUmbrellaWorkspace.tsx:366), and `bee events
    query` is a debugging verb for agents and the lead, not a screen. (needs Brian's sign-off)
  mobile: no surface, by decision — no CLI on the phone; founder provenance stays read-only
    there. (*Amended 2026-09-07 (Andy):* the phone is no longer a read-only observer overall — see
    the mobile lines above.)
  web: no surface, by decision — same. (needs Brian's sign-off)

Tests you must add (red first, then green) — `cargo test -p beekeeper-cli --lib`:
  - `events_query_requires_kinds` — a filter with no kinds is refused locally, exit 1, with the
    verbatim message.
  - `events_query_builds_the_filter_it_was_asked_for` — kinds/authors/#h/since/until/limit map onto
    the exact filter JSON; a UUID channel becomes `#h`.
  - `events_query_refuses_a_malformed_author_before_any_request`
  - `compact_rows_survive_an_event_whose_content_is_not_json`
  - `founder_is_the_genesis_signer_not_the_provider`
  - `founder_is_null_when_the_genesis_event_is_absent_from_the_channel`
  - `a_resume_signer_is_never_reported_as_a_founder`
  - `create_signer_joins_through_the_receipt_command_id`
  - `two_umbrellas_in_one_channel_keep_their_own_founders`

Acceptance:
  cargo test -p beekeeper-cli --lib → report passed/failed counts
  cargo clippy -p beekeeper-cli --all-targets -- -D warnings → clean
  Live, against the dev relay: `bee --format compact events query --kinds 44223 --channel <uuid>
  --limit 20` returns rows, and `bee sessions status --channel <uuid>` names a founder for a session
  whose founder you can independently confirm. Paste both outputs (redacting nothing but content).

Out of scope: `bee events sub` / any live subscription; a write path (`bee events publish`);
  `--search` (NIP-50 belongs to `messages search`); adding a founder column to inbox/roster;
  resolving a founder pubkey to a display name (that is a profile lookup, and a wrong name is worse
  than a key).

Report format: docs/CREW_SESSIONS_PLAN.md §1.2.
```

---

## What this spec deliberately leaves out

- **A crew editor.** The installer writes one roster; changing it means editing the team's crew
  block, which no UI does. Dogfooding this week needs a crew that launches, not one that is
  configurable. *2026-09-08:* the founded session's Team card now composes the lead, the
  bench and the policy **before Start** (`desktop/src/features/coding-sessions/ui/founded/`);
  revising the bench or the policy after Start is still open. *2026-09-10:* that card is now
  the whole front door — every "New coding session" founds the topic on the click and lands
  on the setup card (`CodingSessionFoundedSetupCard.tsx`, same folder), where Solo or Team,
  the name, the prompt, the lead, the bench and the policy are picked; the create dialog is gone.
- **Two builder seats.** The installer mints one agent per pack, so the default roster has one
  builder. A second is a cloned agent plus a crew edit — see above.
- **Publishing `home_role`.** It stays host-local on the record; the wire already carries the
  seat's role, and a second, differently-sourced role on kind:30175 is a second answer to the same
  question.
- **Materializing packs for the channel-agent spawn path.** `docs/CREW_ROLES.md` § Materialization
  records that managed agents in the shared nest materialize nothing; lane A gives them a pack
  *link*, which the coding-session seat path consumes. The nest path stays refused.
- **Web, everywhere; mobile for custody only.** *Amended 2026-09-07 (Andy):* mobile holds the
  paired identity's key and now publishes member-signed session commands and terminal input; what
  stays refused there is anything needing host-local custody (create, seat, install, mint). Web
  remains a read-only observer with no key custody.
- **A CLI path to seat or install.** Custody is host-local; a second minting path is how you get an
  agent the provider cannot resolve.
