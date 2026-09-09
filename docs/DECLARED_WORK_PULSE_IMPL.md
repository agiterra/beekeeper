# Declared work in Project Pulse — implementation contract

Binding scope: `docs/WORK_COORDINATION_VISIBILITY_SPEC.md` (step 3, first
increment). This file fixes the projection, the seam, the read bound and the
surface so three lanes can build against one contract by strict file
ownership. It is an execution contract, not a claim that anything below is
implemented; status belongs in `docs/SESSION_STATE.md`.

## 1. What the section shows, and from where

One new section in the existing project Pulse view, `pulse-declared-work`,
listing **declarations**: each is either

- a **plan** — an active kind 44240 entry of type `plan`, exactly the row
  `PulseEntryRow` renders today (author, age, prose, branch chip, claimed
  areas, session sentence). Label: **"Plan posted"**. Plans are *regrouped*
  into this section; the existing `pulse-entries` list keeps every other entry
  type (note, blocker, handoff, milestone). Superseded plans stay in the
  existing `pulse-superseded` disclosure. No second plan list exists anywhere.
- an **assignment** — one canonically included kind 44244 `assignment`, joined
  from the verified mission gather through the new native projection below.
  Label: **"Assigned"**, plus the evidence labels §3 defines.

The section renders a model it is handed. It fetches nothing, folds nothing,
and never re-verifies on hover, scroll or disclosure.

## 2. Sources and canonical inclusion

| Fact | Source | Canonical decision |
|---|---|---|
| Plan | digest `entries[]`, `type === "plan"`, `active === true` | existing `foldProjectPulseDigest` supersession (same author, `(createdAt, eventId)` order) |
| Session identity, lifecycle, name, newest observation | digest `sessions[]` (`CoordinatedSession`) | existing session coordination fold; `lifecycle` is `open` or `closed` |
| Channel, genesis, founder, active seats and grants | existing mission gather (`readPulseMissionSessions`: 44226 genesis index, relay-signed 44228 acceptance) | unchanged |
| Assignment inclusion, settlement, terminal | `fold_coding_session_team_transactions` over the session's signed 44244 set | `CodingSessionTeamFold.assignments[]` (assignment ids + settlement), `included_event_ids`, canonical terminal |
| Assignment payload | the included assignment event, decoded by `validate_coding_session_team_transaction_envelope` | only events whose id is in `assignments[].assignment_event_id` |
| Reports | included 44244 `report` events whose `assignmentRef` names the assignment | inclusion by the fold; `unseated_reports` marks authors with no active seat |
| Dispositions | included 44244 `verdict` events whose report is a report of this assignment | inclusion by the fold; **settled** only from `settlement.settled` |

Rules the projection must honour (from the spec):

- An assignment the fold excluded is not a declaration. The count of excluded
  44244 events per session is disclosed as a number, never their content.
- Closing a session settles nothing. An unsettled assignment in a `closed`
  session is **unresolved** and stays visible.
- A report is evidence of a report. `status` never reads "done" from a report.
- Settlement is the fold's existing rule: an approving disposition plus the
  assignee's acknowledgement. No new approval requirement.
- The assigned actor is the responsible participant; the assigning author's
  pubkey stays inspectable.
- `verifierRequired` is not threaded into this fold context (same as the
  mission adapter). The projection carries no field that could render that
  `false` as "no verifier required".
- No overlap, no path comparison, no repository inference. `fileOwnership`
  strings render verbatim under the declared-scope label. `codeAreas` render
  as they do today. The two are never compared.

## 3. Native projection wire — `pulse_declared_work`

A **new** Tauri command, separate from `pulse_mission_rows`, because the
mission response decoder refuses unknown top-level keys and its fixture is
pinned byte-for-byte from Rust; the frozen mission contract does not change.

### Request (`buzz-pulse-declared-work-request/v1`)

```jsonc
{
  "schema": "buzz-pulse-declared-work-request/v1",
  "project": "30621:<pubkey>:<slug>",   // echoed for binding
  "channelIds": ["…"],                  // the project's channel floor
  "nowUnix": 1756800960,
  "viewerPubkey": "…" | null,
  "sessions": [ PulseDeclaredWorkSessionInput, … ],   // ≤ 8 per request
  "readErrors": [ { "scope": "…", "message": "…" } ]
}
```

`PulseDeclaredWorkSessionInput` (camelCase, `deny_unknown_fields`):

```jsonc
{
  "sessionKey": "…", "channelRef": "<uuid>", "sessionRef": "<uuid>",
  "genesisRef": "<hex64>", "founderPubkey": "<hex64>",
  "name": "…" | null,
  "lifecycle": "open" | "closed",
  "latestObservationAt": 1756800600 | null,
  "activeSeats": [ { "actorPubkey", "role" } ],
  "activeGrants": [ { "actorPubkey", "grantEventRef", "maySteer", "granted", "acceptedAt" } ],
  "teamEvents": [ <whole signed 44244 events, ascending (created_at, id)> ]
}
```

The same seat/grant shapes as `PulseMissionSeatInput` / `PulseMissionGrantInput`.
A request with more than 8 sessions is refused by name, exactly as the
mission command refuses.

### Response (`buzz-pulse-declared-work/v1`)

```jsonc
{
  "schema": "buzz-pulse-declared-work/v1",
  "viewerPubkey": "…" | null,
  "sessions": [
    {
      "sessionKey": "…", "sessionRef": "<uuid>", "channelId": "<uuid>",
      "name": "…" | null, "lifecycle": "open" | "closed",
      "latestObservationAt": 1756800600 | null,
      "founderPubkey": "<hex64>",
      "terminal": null | { "eventId": "<hex64>", "type": "mission.completed" | "mission.blocked", "at": 1756800000 },
      "unreadable": null | "<the fold's own error sentence>",
      "excludedCount": 0,
      "assignments": [
        {
          "sourceEventId": "<hex64>", "createdAt": 1756790160,
          "assignerPubkey": "<hex64>",
          "assigneeActor": "<hex64>", "assigneeRole": "builder",
          "objective": "…", "brief": "…",
          "branch": "…" | null, "baseSha": "…" | null,
          "fileOwnership": ["…"], "acceptanceSteps": ["…"],
          "supersedes": "<hex64>" | null,
          "reports": [
            { "eventId", "authorPubkey", "createdAt", "summary",
              "branch", "baseSha", "headSha", "files": [], "testCount": 0,
              "deviations": [], "residuals": [], "authorUnseated": false }
          ],
          "dispositions": [
            { "eventId", "authorPubkey", "createdAt", "decision": "approve" | "approve-with-notes" | "changes-requested" | "reject" | "blocked", "reportRef": "<hex64>" }
          ],
          "settlement": {
            "settled": false,
            "governedReportEventId": null, "dispositionEventId": null, "acknowledgementEventId": null
          },
          "status": "unresolved" | "reported" | "settled"
        }
      ]
    }
  ],
  "errors": [ { "scope": "declared:<sessionKey>", "message": "…" } ]
}
```

`status` is derived in Rust, once, so the CLI and Desktop would say the same
word: `settled` iff `settlement.settled`; else `reported` iff at least one
included report names the assignment; else `unresolved`. An `unreadable`
session carries `assignments: []` and its error is also pushed to `errors`;
the section renders it as "records unreadable", never as "no work".

Sessions come back in request order; assignments within a session ascending
by `(createdAt, sourceEventId)`. Ordering for display is the Desktop
projection's job (§5).

### Rust placement

- Pure projection in `crates/buzz-core/src/pulse_declared_work.rs` with tests
  in `pulse_declared_work_tests.rs` (signed events via `Keys::generate()` +
  `EventBuilder`, the same helper shape as `pulse_mission_tests.rs::signed`).
  Entry: `pub fn project_declared_work(sources: &PulseDeclaredWorkSources<'_>) -> PulseDeclaredWorkSession`.
  One `pub mod pulse_declared_work;` line in `crates/buzz-core/src/lib.rs`.
- Tauri adapter `desktop/src-tauri/src/commands/pulse_declared_work.rs` with
  tests in `pulse_declared_work_tests.rs`; `mod` + `pub use` lines in
  `commands/mod.rs`; one handler line in `handlers.rs`. Uses
  `spawn_blocking` like `pulse_mission_rows`.
- A Rust test writes `desktop/src/features/project-pulse/lib/pulseDeclaredWork.fixture.json`
  from the real projection (the mission fixture pattern) and the TS decoder
  test pins it.

## 4. Desktop read: bounded pagination over visible sessions

`pulseMissionSessionRead.ts` gains, additively:

- `pulseDeclaredWorkSessions(digest)`: **every** digest session (open and
  closed), newest `latestObservationAt` first, nulls last, ties by
  `sessionKey` byte order. Returns `{sessionKey, sessionRef, name, lifecycle, latestObservationAt}`.
- `readPulseMissionSessions(input, deps, { records: "team-only" })`: an
  options bag whose default keeps today's behaviour byte-for-byte. With
  `team-only` the gather reads genesis, authority transitions and relay
  receipts, and **only** kind 44244 per session — no 44245, 44246, lifecycle
  or ref-state reads. Its result is the same `PulseMissionSessionInput` with
  those lists empty; the declared-work request builder maps it onto
  `PulseDeclaredWorkSessionInput` and attaches `lifecycle` from the digest.

Pagination constants (exported): `PULSE_DECLARED_WORK_PAGE_SIZE = 8`,
`PULSE_DECLARED_WORK_MAX_PAGES = 4`. Page *n* is visible sessions
`[8n, 8n+8)`; each page is one gather + one native invoke. The first page
loads with the screen; "Show older sessions" loads the next; after page 4
the control is replaced by the disclosure below.

`pulseQueries.ts` gains `usePulseDeclaredWork(coordinate, channelIds, {digest, displayNames})`
built on `useInfiniteQuery`, key
`["project-pulse-declared", coordinate, sortedChannels, visibleSessionKey, viewerPubkey]`
where `visibleSessionKey` encodes the ordered `(sessionKey, lifecycle)` list
(a session appearing, reordering across page boundaries, or closing starts a
new consistent paged read). Timestamp-only changes that preserve this order and
lifecycle do not invalidate the pages. `refetchInterval: 60_000`,
`retry: false`, `enabled: coordinate !== null`. Refetch re-reads **loaded**
pages only. A page error keeps earlier pages' rows and is disclosed. No
module-level cache is added (no new `resetCommunityState` entry); if one is,
it goes into `projectPulseCache.ts` and its reset is wired the same change.

"Check again" on the section calls the digest's and this hook's `refetch`
only. It starts nothing.

## 5. Desktop projection module — `pulseDeclaredWork.ts` (pure, tested)

`projectPulseDeclaredWork(input) -> PulseDeclaredWorkModel` with input
`{ digest, pages: PulseDeclaredWorkResponse[], pageErrors, visibleSessionCount, loadedPageCount, maxPages, pageSize, nowSeconds, viewerPubkey }`.

Output:

```ts
type PulseDeclaredWorkModel = {
  // newest first: plans by createdAt, assignments by createdAt; interleaved
  current: PulseDeclaredWorkRow[];   // active plans + unresolved/reported assignments
  settled: PulseDeclaredWorkRow[];   // settled assignments (collapsed group)
  scan: {
    visibleSessions: number; scannedSessions: number; unreadableSessions: number;
    morePages: boolean;              // another page exists and the cap allows it
    capped: boolean;                 // max pages reached with sessions left
    sentence: string;                // "Scanned 8 of 13 visible sessions, newest first; 5 older sessions not read yet."
  };
  limitations: string[];             // per-page read errors, unreadable sessions, cap
};
type PulseDeclaredWorkRow =
  | { kind: "plan"; entry: PulseDigestEntry; label: "Plan posted" }
  | { kind: "assignment"; label: "Assigned"; session: {...}; assignment: PulseDeclaredAssignment;
      evidence: Array<{ label: "Report submitted" | "Disposition" | "Settled" | "Mission completed" | "Mission blocked" | "Session closed"; detail: string; eventId: string | null }>;
      responsible: { pubkey: string; role: string };
      assignedBy: string;
      dedupeKey: string };            // `${channelId}:${sessionRef}:${sourceEventId}`
```

Dedupe key includes channel, session and source event. The community and
project are already fixed by the query key. Two pages never repeat a session
because pages are disjoint slices of one sorted list; if a refetch reorders
sessions across page boundaries, the model dedupes by key and keeps the newer
`createdAt`.

"No declared work" is allowed only when every loaded page succeeded, no
session was unreadable, no plan is active, and `capped === false`.

## 6. Surface — `PulseDeclaredWorkSection.tsx`

Test ids: `pulse-declared-work`, `pulse-declared-scan`, `pulse-declared-row`
(with `data-kind="plan"|"assignment"`), `pulse-declared-label`,
`pulse-declared-responsible`, `pulse-declared-scope`,
`pulse-declared-branch`, `pulse-declared-evidence`, `pulse-declared-details`
(collapsed `<details>` holding brief, acceptance steps, ids),
`pulse-declared-open-session`, `pulse-declared-settled` (collapsed group),
`pulse-declared-more` (load next page), `pulse-declared-limitations`,
`pulse-declared-recheck`.

Row content (assignment): label chip; objective; "Assigned to {name} as
{role}" with the actor's avatar/name from `authorNames` (hex fallback) and
"by {assigner}" inspectable; declared paths verbatim (`fileOwnership`), or
"No declared paths"; branch/base when reported, else "Branch not reported";
record age; the session's name and lifecycle ("Session closed" is evidence,
never settlement); evidence lines; a collapsed details block with the brief,
acceptance steps, source event id, session/genesis ids.

Row content (plan): unchanged `PulseEntryRow` under the label chip.

Source navigation: "Open session" calls `onOpenSession(targetKey)` for the
session's newest generation, which `ProjectPulseScreen` now wires to
`useAppNavigation().goCodingSession(channelId, generationId)` with
`generationId = buildCodingSessionTranscriptGenerationId(channelId, generation.providerAuthorityPubkey, decodeCodingSessionTargetKey(targetKey))`.
A session with no generation on the wire shows "No execution recorded to
open". Navigation publishes nothing.

Typography: rem tokens only (`text-sm`, `text-xs`, `text-2xs`). Zoom 250%
and 640px width must keep the label, objective and responsible line readable
with wrapping, no clipped controls.

## 7. Lanes and file ownership (nobody commits)

- **Lane P — projection wire (Rust + TS decoder).** Owns
  `crates/buzz-core/src/pulse_declared_work.rs`, `…_tests.rs`, the `lib.rs`
  mod line; `desktop/src-tauri/src/commands/pulse_declared_work.rs`,
  `…_tests.rs`, `commands/mod.rs` lines, `handlers.rs` line;
  `desktop/src/features/project-pulse/lib/pulseDeclaredWorkWire.ts` (+ test)
  — strict decoder mirroring `pulseMissionWire.ts` — and
  `invokePulseDeclaredWork.ts` (+ test); the JSON fixture.
- **Lane Q — read and projection (TS).** Owns `pulseMissionSessionRead.ts`
  (+ test), `pulseQueries.ts` (+ test), new `pulseDeclaredWork.ts` (+ test).
  Codes against §3 types as written here; Lane P's decoder exports
  `PulseDeclaredWorkResponse` and `PulseDeclaredWorkSessionInput` by exactly
  those names.
- **Lane U — surface.** Owns new `ui/PulseDeclaredWorkSection.tsx` (+ test),
  `ui/ProjectPulseView.tsx` (+ test: regroup plans, mount section),
  `ui/ProjectPulseScreen.tsx` (hook + navigation wiring), new
  `lib/pulseSessionRoute.ts` (+ test), `desktop/tests/e2e/project-pulse-declared-work.spec.ts`
  + its `mockPulseDeclaredWork` helper in `tests/e2e/helpers/`, registration
  in `playwright.config.ts`. Two owners, narrow width, zoom, stale evidence,
  hash-distinct screenshots on port 4176.
- **Lane R — independent review.** Reads everything, edits nothing. Checks
  provenance labels, multi-repository isolation (no inferred conflict),
  incomplete/capped reads, old declarations vs current, accessibility, and
  that no new gate or approval step was introduced.

## 8. Gates

`cargo test -p buzz-core pulse_declared_work`, `cargo test --manifest-path desktop/src-tauri/Cargo.toml pulse_declared_work`,
`cargo clippy --all-targets` for both, `cargo fmt --check`; `pnpm check`
(biome, tsc, px-text), focused `node --test` files, then the full desktop
unit suite; Playwright Pulse specs on `BUZZ_E2E_PORT=4176`. Separate Cargo
target dir, `-j 3`.

## 9. Stated limits

- Mock-bridge E2E proves the surface against fixture bytes, not cross-machine
  delivery or native responsiveness; the two-machine runbook §5 is root's.
- Page 0 re-reads the 44244 set of open sessions the mission read also reads
  (bounded: one query per session, cached 60 s). A shared per-session event
  cache is a later optimisation.
- Display names come from the digest's author map; an assignee who never
  wrote an entry renders as short hex.
