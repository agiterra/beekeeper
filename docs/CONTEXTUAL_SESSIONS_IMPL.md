# New session in this workspace — implementation contract

Brief: `BEEKEEPER-FABLE-CONTEXTUAL-SESSIONS-BRIEF.md` (Desktop). Product
authority: `VISION_COLLABORATION.md`, `docs/SESSION_VISION.md`. This is an
execution contract; status belongs in `docs/SESSION_STATE.md` (root's).
Branch `work/contextual-sessions-fable`, base `f7b9d5542`.

## 0. What exists, and what is missing

The launcher is already the whole creation pipeline, and it already reuses a
directory correctly when the worktree toggle is off:
`useNewCodingSessionLaunchSubmit.ts:146-168` skips `createCodingSessionWorktree`
and names the directory verbatim; `useNewCodingSessionCreate.ts:523-533` stages
the per-command hint the provider resolves its cwd from. Nothing needs a second
pipeline.

Three things are missing:

1. **No caller can open the launcher pre-seeded.** The directory and the
   worktree toggle are `React.useState` initializers inside the form
   (`NewCodingSessionLaunchForm.tsx:395-396`, reserved), and no prop reaches
   them. `projectContext.defaultWorkdir` is not it: it enters through
   `NewCodingSessionWorkdirField`'s `fallbackPath`, which sits *below* the
   project's and channel's remembered directories in
   `preferredWorkdirPrefill` (`NewCodingSessionWorkdirField.tsx:28-46`), so a
   channel with a remembered folder would silently win over the workspace the
   person just chose.
2. **An explicit one-off reuse would become the remembered default.** On
   submit the reserved hooks call `recordCodingSessionWorkdirUse(rememberWorkdir
   ?? workdir)` (MRU) and `stageCodingSessionCreateHint({… projectRef})`, whose
   Rust side sets the project's default when it has none yet
   (`workdir_store.rs` `stage_hint_for_project`). The brief forbids exactly
   this: "A remembered canonical checkout must not be replaced by this explicit
   session override." Passing `rememberWorkdir: null` does **not** suppress it —
   the create hook falls back to `input.workdir`.
3. **No session → directory answer for the menu.** `listCodingSessionSeatWorktrees`
   returns recorded seat trees for named sessions with `path`, `branch`,
   `exists`; nothing composes that into "can this computer honour a new session
   in this session's workspace".

## 1. The seam root implements (three reserved files, small)

Type declared in `NewCodingSessionDialog.tsx` (mine, not reserved) and imported:

```ts
export type NewCodingSessionWorkspaceReuse = {
  /** Absolute path on this machine, already verified by the caller. */
  path: string;
  /** Recorded or live branch, display only. Never published. */
  branch: string | null;
};
```

1. `NewCodingSessionLaunchForm.tsx`
   - prop bag (`:113-122`): `workspaceReuse?: NewCodingSessionWorkspaceReuse | null` (default `null`).
   - `:395` `const [workdir, setWorkdir] = React.useState(workspaceReuse?.path ?? "")`.
   - `:396` `const [useWorktree, setUseWorktree] = React.useState(workspaceReuse === null)`.
   - the submit call gains `rememberWorkspace: workspaceReuse === null`.
2. `useNewCodingSessionLaunchSubmit.ts` — input gains `rememberWorkspace?: boolean`
   (default `true`); it is threaded to the create's `submit` unchanged. No other
   behaviour changes: the reuse path already works when `useWorktree` is false.
3. `useNewCodingSessionCreate.ts` — `submit` input gains `rememberWorkspace?: boolean`
   (default `true`). When `false`: skip `recordCodingSessionWorkdirUse`
   entirely, and call `stageCodingSessionCreateHint` with `projectRef: null`
   so the one-off directory can neither enter the MRU nor become a project's
   first default. The per-command hint still binds the cwd, which is what the
   provider reads.

Nothing else in the reserved files changes; `newCodingSessionModel.ts` is not
involved. The edits are textually disjoint from root's in-flight
workdir-failure repair (which touches `editRequested`/`repairWorkdir`, not the
workdir/worktree state).

## 2. Resolution contract (mine)

`codingSessionWorkspaceReuse.ts` (new, pure):

```ts
export type WorkspaceReuseAvailability =
  | "available"        // a recorded directory on this computer, present now
  | "missing"          // recorded here, but the directory is gone
  | "unrecorded"       // this computer recorded no directory for this session
  | "elsewhere";       // the session's execution runs on another provider

export type WorkspaceReuseResolution = {
  availability: WorkspaceReuseAvailability;
  /** Only for "available"/"missing". Absolute, local, never published. */
  path: string | null;
  /** Recorded branch (creation-time fact) or the live head when read. */
  branch: string | null;
  branchSource: "recorded" | "live" | null;
  /** Session refs that also recorded a tree at this exact path — never a registry. */
  alsoHere: readonly string[];
  /** One sentence naming what is known, in this app's idiom. */
  sentence: string;
};

export function resolveWorkspaceReuse(input: {
  sessionRef: string;
  rows: readonly SeatWorktreeRow[];      // from listCodingSessionSeatWorktrees
  validation: { exists: boolean; isDir: boolean } | null;  // validateCodingSessionWorkdir
  executionIsLocal: boolean | null;      // provider pubkey comparison; null = unknown
  liveBranch?: string | null;
}): WorkspaceReuseResolution;
```

Rules, all fail-closed:

- A row for this `sessionRef` with `exists: true` and a validation saying
  `isDir` → `available`. `exists: false` or validation failing → `missing`.
- No row → `unrecorded`, whatever else is true. A session launched with the
  worktree toggle off leaves no durable session→path record, so "unrecorded"
  is the honest answer, never "the channel's folder".
- `executionIsLocal === false` → `elsewhere` and **no path**, even if a row
  exists. `null` (unknown) never becomes `elsewhere`; it degrades to whatever
  the rows say, and the sentence says the execution's location is unknown.
- `alsoHere` lists only session refs whose rows carry the same `path`, drawn
  from the same single call. An empty or failed read renders nothing.
- Sentences reuse the house idiom — an absence on *this* computer, never a
  claim about another machine: "No such directory on this computer.",
  "This computer recorded no directory for this session.", "This session's
  execution runs on a provider this computer does not hold."

`useCodingSessionWorkspaceReuse(sessionRef, channelId)` (new hook, in
`features/coding-sessions/hooks/`) performs at most: one
`listCodingSessionSeatWorktrees([{sessionRef, …}])`, one
`validateCodingSessionWorkdir(path)` when a row was found, one
`listCodingSessionWorktreeBranches({workdir})` for the live head (optional,
failure → keep the recorded branch), and the local provider-status comparison
the app already does. It reads on menu open, not on render, and never on hover
or scroll. Both entry points consume this one hook so they cannot drift.

## 3. Entry points and behaviour (mine)

- **Where.** The sidebar session row's context menu (`ProjectChildRowItem.tsx`,
  Radix `ContextMenu`) and the open session's header overflow
  (`CodingSessionHeaderOverflow.tsx`, its `OverflowItem[]` data array). One
  label in one constant: **"New session in this workspace"**.
- **Discoverability and keyboard.** The sidebar row's context menu is
  right-click only today and is mounted only when another action is available
  (`ProjectChildRowItem.tsx:270`); widen that condition so the item is always
  reachable, and add a keyboard route (`Shift+F10`/`ContextMenu` key opens the
  same menu on the focused row). The header overflow is already a focusable
  button list.
- **The item is always present, and never claims.** Availability changes what
  it opens and what its secondary line says, not whether it exists — the brief
  requires the unavailable case to explain itself and offer the ordinary
  folder selection. `available` → the directory's base name and branch;
  `missing`/`unrecorded`/`elsewhere` → the resolution's sentence.
- **The click opens a draft and nothing else.** No signing, no
  start/resume/stop, no branch change, no grant change, no directory created,
  no worktree planned. Opening the launcher is the whole effect.
- **What the draft says.** With a resolved workspace: the absolute path, the
  branch (labelled `recorded` or `on disk now`), and one plain sentence —
  **"New conversation; uses these files."** — plus "Includes uncommitted
  changes already in this folder." It must not say isolated, forked,
  inherited, continued, or taken over. Without one: the sentence from the
  resolution and the ordinary folder picker, with nothing seeded.
- **Worktree creation is off** for a reuse draft, and the person may still turn
  it on or change the folder before submitting; doing so is an ordinary launch.
- **Nothing is remembered.** A reuse submit carries `rememberWorkspace: false`
  (§1). A later ordinary "New session" for that project still offers the
  canonical checkout.

## 4. Store and dialog plumbing (mine)

`newCodingSessionDialogStore.ts`: `NewCodingSessionRequest` gains a third arm
`{ kind: "workspace", channelId: string | null, projectId: string | null,
sessionRef: string, workspace: NewCodingSessionWorkspaceReuse }`, an
`openNewCodingSessionDialogInWorkspace(input)` opener, and the matching branch
in `parseNewCodingSessionRequest` — a field absent from that parser is dropped
on reload, so its test pins the round trip. `NewCodingSessionDialogHost.tsx`
threads `workspaceReuse` into `NewCodingSessionDialog`, which declares the type,
passes it to the form, and varies the title ("New session in this workspace").

## 5. Lanes and file ownership (nobody commits)

- **Lane E — entry points and draft copy.** `ProjectChildRowItem.tsx`,
  `CodingSessionHeaderOverflow.tsx` and its callback in
  `CodingSessionWorkspace.tsx`, new `ui/CodingSessionWorkspaceReuseSummary.tsx`
  (the draft's disclosure block) + tests, new
  `lib/codingSessionWorkspaceReuseCopy.ts` (every sentence, one place) + tests.
- **Lane W — resolution and plumbing.** New
  `lib/codingSessionWorkspaceReuse.ts` (+ test), new
  `hooks/useCodingSessionWorkspaceReuse.ts` (+ test),
  `newCodingSessionDialogStore.ts` (+ its test),
  `NewCodingSessionDialogHost.tsx`, `NewCodingSessionDialog.tsx` (type +
  pass-through + title), `shared/api/tauriCodingSessionWorkdirs.ts` only if the
  declared state must widen.
- **Lane V — integration evidence.** `desktop/tests/e2e/coding-session-workspace-reuse.spec.ts`
  + its helper, `playwright.config.ts` registration, and the mock-bridge
  handlers for `get_coding_session_workdir_state`,
  `validate_coding_session_workdir` and `record_coding_session_workdir_use`
  **added in the spec's own init script** (the bridge file is over the size
  ratchet; opt-in-or-throw stays the house rule).
- Reserved, never edited by any lane: `NewCodingSessionLaunchForm.tsx`,
  `useNewCodingSessionLaunchSubmit.ts`, `useNewCodingSessionCreate.ts`,
  `newCodingSessionModel.ts` and their tests. No provider, authority, claim or
  membership file belongs to this feature.

## 6. Acceptance

A session whose recorded tree is `repo-wt-a` with an uncommitted marker: the
menu opens a draft on `repo-wt-a` with worktree creation off; submit names that
exact directory at the execution seam and the marker is readable there; the
first conversation is untouched. A later ordinary project "New session" still
offers `repo`, not `repo-wt-a`. Missing directory, foreign host, unrecorded
session, the person changing the folder before submit, a cancelled draft, a
repeated menu click, and a community/project switch each produce their own
truthful state with no leaked workspace. Behaviour assertions, not wiring
inspection. Screenshots: standard, narrow, keyboard focus at 250% zoom, and an
unavailable state.

## 7. Gates and limits

Focused unit tests, the feature's browser spec on port 4176 (never root's
4177), `pnpm tsc --noEmit`, `pnpm check`, file-size ratchet. Root's full smoke
has 165 unrelated failures; this slice claims only its own focused runs. Mock
bridge coverage is never called native Windows proof. The path stays a local
launch hint: it is never added to a published event field.
