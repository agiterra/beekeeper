# Singularity — Surfaces

**Designer:** Banksy (`designer` seat). **Base:** `19ad1b97`.
**Mock:** `/Users/brian/Downloads/singularity/Singularity.png`.
**Design doc:** `beekeeper-singularity-ui-remodel.md` (three planes, five levels,
"does the viewer need to know this?", reversible compression,
participant-over-provider, contextual lead, compact handoffs — adopted as
written).
**Mission reference:** `review-2026-08-28/ref/t3-workflow-card-{header,expanded}.png`.

**Instrument.** Every state below marked *seen* was reached in the desktop app
on the E2E mock bridge, from this worktree:

```
cd desktop && pnpm build:e2e
npx playwright test --project=smoke coding-session-surface-host-screenshots
npx playwright test --project=smoke coding-session-reachability
```

Captures: `desktop/test-results/coding-session-surface-host/0{1..9}-*.png`, nine
distinct hashes. States I could not reach are written **not reached**, with why.

**Read this in ten minutes:** §1 (what changes), §2 (the wire table), §11 (every
number in the mock, sourced or unknown), §15 (what I found by driving), §16 (the
two lanes). §§3–10 are the per-element law the lanes build to; skim their
rulings (the indented blocks) and skip the rest.

---

## 1. The ruling, in one page

The umbrella surface is not missing panels. It is missing **weight**. Driving it
(`01-merged-active-work.png`), every fact is present and every fact is the same
size: the team's disposition is 11px grey fine print at the top
(`Claude Code · sonnet · live · last turn just now   Codex · gpt-5.6-sol · idle
· last turn just now`), while a task list the agent wrote itself is a large
white card covering a third of the stream. A person reads the small text to
learn the temperature and reads the big card to learn nothing. That is the whole
bug, and it is a design bug, not a data bug.

Five moves, in order of how much they change the feel:

1. **The disposition strip becomes the participant status bar.** Same facts,
   same model, ten times the weight — a row of chips, each `name · role` over a
   state word, live ones breathing. It is already the honest fact
   (`codingSessionUmbrellaModel.ts:589`); it was rendered as a footnote.
2. **The active-work card becomes a 44px live activity bar.** One line per
   working seat, above the composer, never over the stream.
3. **The inspector gets built** — goal, plan, changes, files, tests, team — so
   nobody reads the transcript to learn the state. Three of its six panels have
   no signed source and say so out loud.
4. **A Mission card lands in the stream** where the lead's Pulse chain says a
   mission exists: brief in place, phases as pills, per-seat model · tokens ·
   tools, one footer line for the session.
5. **Brief / Live / Trace** is a lens on the same signed items, and it is
   forbidden from hiding a blocker.

**Naming.** Singularity is the surface. `sessionRef`, umbrella, execution,
generation stay exactly what they are on the wire, in `bee`, and in code. No
copy in this spec assumes a lead exists; where a lead does exist it is a `role`
on a 44223 record and nothing more.

**Two lines I am overriding the mock on**, because the mock's word is not what
the wire knows:

| Mock says | Surface says | Why |
|---|---|---|
| `Reviewer · Reviewing` | `Reviewer` / `live` / `reviewing the dispatch change` | "Reviewing" is not a wire state. State comes from 44223 + lease; the activity phrase is the seat's own plan text, rendered as the seat's words, on its own line. |
| `+ Member` | `Add provider…` | The control attaches a provider execution. "Member" would claim a person joined. |

---

## 2. Wire table — every fact this surface may state

Per `skills/wire-sources-for-surfaces`. **A panel that cannot name its row shows
the unknown copy, never a number.**

| # | Fact | Signed source | Copy when it has not arrived |
|---|---|---|---|
| W1 | Liveness | 44223 `status` demoted by the ephemeral lease (`deriveCodingSessionWorkspaceStatus`, `codingSessionWorkspaceModel.ts:292`) | `live` · `idle` · `released` · **`No provider answering`** + `last reported Idle 2h ago` |
| W2 | Turn stages | 44224 `turn_queued`/`turn_started`/`turn_degraded`/`turn_dropped`/`turn_refused`/`interrupt_delivered` | `no receipt yet` |
| W3 | Dispositions | 44240 Pulse, `pu-type` ∈ `plan`\|`milestone`\|`note`\|`handoff`\|`blocker`, chained by `supersedes` | `no pulse yet` — rendered as a claim **by its author**, never as a verdict |
| W4 | Authority | 44228 `grant-operator` + roster fold | `View only — ask the session owner for collaborator access` |
| W5 | Goal | 44227, `d=sessionRef` | `No goal set` |
| W6 | Founder | 44226 genesis signer; identity is the genesis **event id** | never unknown |
| W7 | Story | 44225 transcript items | `no turn observed` |
| W8 | Seats | 44223 `agentRef` + `role`; display name from kind-0 | `unseated`; never a pubkey |
| W9 | Hires | 44221 `session.hire` + 44224 receipt (`HIRE_MODEL_NOT_OFFERED`, `HIRE_STALE`) | `hire not answered` |
| W10 | Seat plan | 44225 `plan` item / `update_plan`/`todo` tool item (`codingSessionTaskModel.ts:58`) — **the seat's own plan, not an accepted one** | `No plan published` |
| W11 | Observed changes | 44225 tool items with file edits (`deriveCodingSessionChangedFiles`) | `No observed changes yet` / `Observed in transcript activity` |
| W12 | **Tokens, tool calls, context** | 44225 terminal `result` item, `usage` block — `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `toolCalls`, `contextWindow` (`coding_session_payload.rs:1196`); occupancy from the `context_window_updated` item (`coding_session_payload.rs:1268`) | `tokens not reported` / `tools not reported` / `window unknown` |
| W13 | Model / runtime | 44223 `model`, `runtime` | `model unknown` |
| W14 | **Tests** | **none today** | `No test report yet` |
| W15 | **Accepted plan** (the lead's acceptance steps) | **none today** — the brief's `Acceptance:` line is prose | `No accepted plan published` |
| W16 | Mission brief | the 44220 turn command the seat was opened with, echoed as its first 44225 item | `no brief on the wire` |

**W12 corrects my brief.** The brief lists tokens and tool calls under "requires
new wire artifact". They landed in ledger item 89(b) and are on `main` at
`19ad1b97`: the `usage` block rides the 44225 `result` item, and
`bee sessions status` already prints a context column from it
(`crates/buzz-cli/src/commands/sessions/crew_cmds.rs:884-933`). The T3
reference's `opus-5[1m] · 193k tok · 99 tools` is **sourceable today**. Nothing
in this spec fakes it from transcript length. See §15(a).

Three rules the lanes inherit: absence ≠ zero; one row = one voice (two panels
showing W1 use the same word); a claim (W3, W10, W16) is rendered with its
author's name attached, an observation (W1, W2, W7, W12) is not.

---

## 3. Region A — Header

**Mock:** `Singularity.png → header → "HiringTest" + "Singularity" pill + goal
line + "Changes 12" + "…"`.

### A1 — Singularity name
- cite: `Singularity.png → header → "HiringTest"`
- replaces: `CodingSessionHeader.tsx:205` (`<h1>{title}</h1>`) — kept, unchanged
- reads: 44229 session name, else umbrella title (`umbrella.title`)
- copy: the name verbatim; when neither exists → `Untitled Singularity` (today's
  `"Coding session"` at `CodingSessionHeader.tsx:164` is replaced)
- walk: open any umbrella; rename via the pencil (founder only) and read the
  header

### A2 — Goal line
- cite: `Singularity.png → header → "Implement session dispatch changes"`
- replaces: `CodingSessionHeader.tsx:232-257` — kept; today the goal already
  wins the context line
- reads: **W5**
- copy: the goal verbatim; empty → `No goal set` (never the channel name and
  never `2 executions` in its place — see A5)
- walk: seed a 44227 for the sessionRef; clear it; confirm the line changes and
  the inspector's Goal panel says the same words (one row, one voice)

### A3 — Lifecycle chip `● Running`
- cite: `Singularity.png → header → "● Running"` (first chip of the status row)
- replaces: `CodingSessionHeader.tsx:264-298` (status badge) and
  `CodingSessionUmbrellaWorkspace.tsx:417` (`statusLabelOverride`), which today
  renders the aggregate `2 agents · 1 working` — **seen**, `01`
- reads: **W1**, folded across executions by `deriveUmbrellaStatus`
  (`codingSessionUmbrellaModel.ts:486`), **demoted per execution first**
- copy: `Running` · `Idle` · `Ended` · `Needs attention` ·
  `No provider answering`. Detail clause preserved verbatim:
  `last reported Idle 2h ago` (`codingSessionWorkspaceStatusDetail`)
- walk: `coding-session-reachability` spec, test 1 — **seen**: badge reads
  `No provider answering · last reported Idle 2h ago`. Then the umbrella case:
  seed two executions, expire both leases, assert the chip does **not** read
  `Running`

> **Ruling.** `deriveUmbrellaStatus` reads raw 44223 today, so an umbrella all
> of whose providers are gone can still paint `Running`. The lane threads the
> demoted status in before folding. A green dot over a dead team is the exact
> bug class this project treats as a crash.

### A4 — `Observed changes 4`
- cite: `Singularity.png → header → "Changes 12"`
- replaces: `CodingSessionHeader.tsx:381-414` surface tab (label already
  `Observed changes`, `CodingSessionUmbrellaWorkspace.tsx:241`) — **seen**, `04`
- reads: **W11**
- copy: `Observed changes` + count. Never the bare word `Changes`: the number is
  what the transcript showed, not a workspace diff. Zero → the control renders
  with no count, not `0`
- walk: `03`→`04` in the surface-host spec; open the tab and read the footer
  `Observed in transcript activity`

### A5 — Execution count `2 executions`: **dropped**
- `2 executions` (`codingSessionUmbrellaGenerationLabel`,
  `codingSessionWorkspaceModel.ts:149`) — **seen** under the title in `01`.
  Dropped from the header: the participant bar (§4) counts itself, larger and
  with names. The label survives untouched inside the provenance popover, where
  `generation 2 · 1 earlier` is the only place it is load-bearing.

### A6 — `Singularity` pill: **dropped**
- The sidebar section already reads `SINGULARITIES` and the window shows one.
  Delete a row before you style it.

### A7 — `…` overflow
- cite: `Singularity.png → header → "…"`
- replaces: the current header's flat run of buttons —
  `CodingSessionHeader.tsx:445-529` (Add provider, Stop all, Close, Reopen,
  Export, Pop out)
- reads: n/a (controls)
- copy: menu items verbatim — `Add provider…`, `Stop all (N)`,
  `Close session`, `Reopen session`, `Export transcript`, `Pop out`. `Stop all`
  keeps its sentence verbatim: *"Stop every live seat in this session. The
  session stays open; a stopped seat cannot be resumed."* Absent, never
  disabled, for a non-founder (`CodingSessionHeader.tsx:76-79`)
- walk: open as founder → menu lists Stop all with a count; open as a granted
  collaborator → the item is not in the menu

---

## 4. Region B — Participant status bar

**This is the disposition strip, promoted.** Mock: `Singularity.png →
participant status bar → [● Running] [K Keystone · Lead] [B Builder · Working]
[R Reviewer · Reviewing] [+ Member]`.

### B1 — Seat chip
- cite: `Singularity.png → participant status bar → Keystone chip`
- replaces: `CodingSessionHeader.tsx:575-622` (`CodingSessionDispositionStrip`,
  a `text-2xs` `<ul>`) — **seen**, `01`, top line
- reads: **W8** (name, role), **W1** (state)
- copy, two lines per chip:
  - line 1 `Keystone · Lead` — from `formatCodingSessionExecutionLabel`; when
    `agentRef` is null → `Codex · gpt-5.6-sol` (runtime · model, today's
    behaviour, **seen**); when seated but the kind-0 profile has not been read →
    the role and runtime, **never a pubkey**
  - line 2, one of: `live` · `idle` · `released` · `No provider answering` ·
    `Needs attention` · `Disconnected`
  - line 3, only while `live` and only when a plan snapshot exists: the
    in-progress task text, quoted from the seat, ≤ 48 chars, e.g.
    `reviewing the dispatch change`. Absent → no third line (never `working…`)
  - hover/`title`: `last turn 4m ago`, or `no turn observed`
- walk: surface-host spec → the two chips read `Claude Code · sonnet / live` and
  `Codex · gpt-5.6-sol / idle`. Then seed a seated create (`agentRef` + `role`)
  and confirm the chip reads `Keystone · Lead`. **not reached with this
  instrument: a seated chip** — the surface-host fixtures publish unseated
  executions; the lane adds `agentRef`/`role` to the fixture.

> **Feel.** Live chips carry the existing `coding-session-agent-breathe`
> animation (`CodingSessionAgentFocus.tsx:90`) at chip scale; nothing else
> moves. Quiet is still. `No provider answering` is the only chip that takes the
> destructive tone — red is for a fact nobody is answering for, not for
> identity. Reduced-motion drops the breath and keeps the dot.

### B2 — Aggregate chip `2 agents · 1 working`: **dropped**
- `CodingSessionAgentFocus.tsx:212` / `umbrellaAgentStatusSummary`
  (`CodingSessionUmbrellaWorkspace.tsx:896`) — **seen**, `01`. A summary of a row
  the eye already reads. Its *focus* function (fold the stream to one seat)
  survives as B3.

### B3 — Focus, on the chip itself
- cite: mock has no equivalent — **new surface** (folding is ours)
- replaces: `CodingSessionAgentFocus.tsx:137-188` (popover list)
- reads: n/a (a lens)
- copy: chip `aria-label` `Focus Keystone · Lead — live`; when focused, the
  existing notice bar verbatim: `Viewing Codex · gpt-5.6-sol` with a clear
  control — **seen**, `02`
- walk: click a chip → stream folds every other seat to a one-line summary
  (**seen**, `02`); click again → unfolds

### B4 — `+ Member` → `Add provider…`: **overridden**, see §1. Moves into A7.

---

## 5. Region C — Main stream

**Mock:** `Singularity.png → main stream → six entries, 5:05 PM to 5:17 PM`.

### C1 — Level-2 narrative entry
- cite: `Singularity.png → main stream → "Builder · Working / Implementing
  dispatch-arm extraction."`
- replaces: `CodingSessionUmbrellaTurnBlock.tsx:113-247` +
  `CodingSessionTranscript.tsx` — kept, restyled
- reads: **W7**; the state word beside the name is **W1**, not a client guess
- copy: `<name> · <state>` where state ∈ the W1 vocabulary. The mock's per-entry
  type words (`Working`, `Test checkpoint`, `Milestone`, `Review complete`) are
  **not** wire facts on a transcript item — see C4
- walk: surface-host spec, `01`

### C2 — Execution bundle disclosure
- cite: `Singularity.png → main stream → "▸ 11 execution events" and
  "Terminal 4 · Read 2 · Edit 3 · Search 2"`
- replaces: the per-tool rows the transcript renders today
  (`CodingSessionTranscriptParts.tsx`) — **seen**, `01`, `Edited
  CodingSessionSurfaceHost… +1 −1 1.0s`
- reads: **W7** — the count is a count of signed 44225 tool items, and the
  per-verb breakdown is their classified descriptors
  (`agentSessionToolClassifier.ts`)
- copy: collapsed `11 execution events`; with the breakdown
  `Terminal 4 · Read 2 · Edit 3 · Search 2`; expanded, the existing per-item
  rows unchanged. Never `Ran Terminal` — the repeated verb is dropped per the
  design doc. **1 event → `1 execution event`** (singular)
- walk: expand one bundle; confirm the count equals the rows revealed. That
  equality is the reversibility contract and gets a test

### C3 — Compact handoff row
- cite: `Singularity.png → main stream → "B → R  Builder → Reviewer · Review
  requested / Dispatch extraction · 3 files"`
- replaces: `CodingSessionUmbrellaTurnBlock.tsx:175-203` (handoff chip) — today
  the chip sits *inside* the recipient's block; ledger 77(b) says the sender's
  panel is canonical and the recipient shows a reference
- reads: **W7** (the handoff prefill is a real 44220 turn carrying the quote) —
  and **W3** when a 44240 `handoff` entry exists for the same pair
- copy: `Builder → Reviewer · Review requested`, second line the quote's first
  line truncated. When the source is not in view, today's honest string is kept
  verbatim: `source not in this view`
- walk: use `Send to…` on a completed block (`CodingSessionUmbrellaTurnBlock.tsx:284`),
  send, and read the resulting row. **not reached: the two-panel rendering of
  ledger 77(b)** — needs two live seats; the fixture in lane 1 seeds it

### C4 — Entry type words (`Test checkpoint`, `Milestone`, `Review complete`)
- cite: `Singularity.png → main stream → the amber "Test checkpoint" and green
  "Milestone" labels`
- replaces: nothing — **new**
- reads: **W3 only.** A transcript item carries no type. The *only* signed
  source for "this was a milestone" is a 44240 Pulse entry by that author
- copy: rendered as `Keystone · milestone` with the author named, using the
  existing Pulse type treatment (`PulseEntryRow.tsx:41-53`: blocker warned and
  bordered, handoff/plan/milestone/note plain). **A turn with no Pulse entry
  gets no type word at all** — it is a turn, and the stream says so
- walk: `bee pulse update --kind milestone --text "…"` from a seat, then read
  the stream. **not reached** — no Pulse fixture in the desktop mock bridge
  today; lane 1 adds one

> **Ruling.** This is where the mock is most seductive. Promotion by "the model
> said checkpoint" is client classification of prose, and prose classification
> is how a comfortable guess becomes a badge. Types come from 44240 or the
> entry has no type.

### C5 — Attention promotion (Level 1)
- cite: `Singularity.png` has no Level-1 entry; design doc §"Level 1"
- replaces: nothing — **new**
- reads: **W3** `blocker`, **W2** `turn_refused` / `turn_dropped` /
  `turn_degraded`, **W9** hire refusals
- copy, verbatim, each full-width with the destructive accent:
  - blocker → `Blocked — <pulse text>` + `Keystone · blocker · 4m ago`
  - `turn_refused` → `This turn was refused. <reason from the receipt>`
  - `turn_dropped` → `This turn was dropped before it ran.`
  - `turn_degraded` → `Steer was degraded to a boundary turn — the provider has
    no native steer.` (ledger 80(c))
  - hire refusal → `The hire was refused: <code>. <sentence>` e.g.
    `HIRE_MODEL_NOT_OFFERED`
- walk: seed a 44224 with each receipt type and assert each renders full-size in
  **all three density modes** (§9). **not reached** — needs receipt fixtures;
  named in lane 1's red-first list

### C6 — Empty stream
- cite: `Singularity.png` has no empty state — **existing surface, kept**
- replaces: `CodingSessionUmbrellaWorkspace.tsx:754-769`
- reads: **W7**
- copy: existing verbatim — `No activity in this session yet.`
  (`CodingSessionUmbrellaWorkspace.tsx:764`). Distinguished from *quiet*: a seat
  that has said nothing reads `no turn observed` in its chip, not `idle 0s`
- walk: open a created-but-silent umbrella

### C7 — Lifecycle rows
- cite: `Singularity.png` has none; today's `Codex · gpt-5.6-sol joined this
  session` — **seen**, `01`, where the active-work card partly occludes it
- replaces: `CodingSessionUmbrellaWorkspace.tsx:781-796`
- reads: **W7**/**W8**
- copy: kept verbatim — `<label> joined this session`,
  `<label> started generation <n>`
- walk: seed a second execution and watch the row appear (it is legible once the
  card becomes a bar, §7)

---

## 6. Region D — Inspector

**Mock:** `Singularity.png → inspector → tabs [Inspector | Context]; sections
CURRENT GOAL, PLAN, CHANGES, FILES · 4 modified, TESTS, TEAM`.

The inspector is the state plane. It replaces the surface host's two rails
(`CodingSessionSurfaceHost`, tabs `Agents` and `Observed changes` — **seen**,
`03`/`04`) with one panel of stacked sections, keeping the host's width, sheet
and close behaviour untouched.

### D1 — `CURRENT GOAL`
- cite: `Singularity.png → inspector → CURRENT GOAL`
- replaces: `CodingSessionGoalPill.tsx` (the pill above the stream)
- reads: **W5**
- copy: goal verbatim; empty → `No goal set`; founder gets `Set goal` /
  `Edit goal`, others get neither control nor a disabled one
- walk: as A2

### D2 — `PLAN`
- cite: `Singularity.png → inspector → PLAN → five rows, two ✓, one →, two ○`
- replaces: `CodingSessionTaskRail.tsx:40-168` — kept whole, moved into the
  inspector
- reads: **W10**
- copy — and this is the important part — the section header names **whose plan
  it is**: `PLAN · Keystone` (the seat whose snapshot this is). Rows use the
  existing icons. Existing strings kept verbatim: `No plan published` /
  *"Plan and todo updates from this signed session will appear here."*;
  `Plan is empty` / *"The latest signed plan intentionally contains no tasks."*;
  `Plan unavailable` / *"Beekeeper could not read the latest signed plan. The
  session transcript is still available."*; footer `Live from signed session`
  and `Updated <time>`
- walk: **seen**, `01` — the three-row plan in the active-work card is this same
  model. Multi-seat: focus each chip and confirm the header names that seat

> **Ruling.** W10 is a seat's own todo list, not an accepted plan. Labelling it
> `PLAN` without an owner invites reading it as the team's contract. The owner's
> name is what makes it honest, and it costs one word. The *accepted* plan
> (**W15**) is in §13.

### D3 — `CHANGES`
- cite: `Singularity.png → inspector → CHANGES → "+84  ~12  −8" and a
  three-colour proportion bar`
- replaces: `CodingSessionChangesRail.tsx:50-57` footer
- reads: **W11**
- copy: `+84 −8 · 4 files` and, beneath, the disclosure verbatim:
  `Observed in transcript activity`. When any file's counts are null the sum is
  suppressed entirely (`sumKnown`, `CodingSessionChangesRail.tsx:128`) and the
  line reads `4 files · line counts not reported`
- walk: open the inspector on the surface-host fixture (**seen**, `04`: one file,
  `+1 −1`); then seed a second edit whose tool result carries no counts and
  confirm the total is suppressed rather than partial
- **`~12` and the proportion bar: dropped** — see §12

### D4 — `FILES · 4 modified`
- cite: `Singularity.png → inspector → FILES → four rows with per-file counts`
- replaces: `CodingSessionChangesRail.tsx:62-109` — kept whole
- reads: **W11**
- copy: existing verbatim — empty state `No observed changes yet` /
  *"File edits observed in this session's transcript will collect here."*;
  per-row `View` / `Hide`; a file with no diff renders its name with no counts,
  never `+0 −0`
- walk: **seen**, `04`

### D5 — `TESTS`
- cite: `Singularity.png → inspector → TESTS → "3 / 3 passing" + a full green
  bar + "100%"`
- replaces: nothing — **new**
- reads: **W14 — no signed source today.**
- copy, always, until W15/W14 exist:
  `No test report yet` /
  *"Nothing on the wire reports tests. A seat's written report is prose in its
  turn — Beekeeper will not count it."*
- walk: open the inspector on any session, including one whose seat has just
  written "3/3 passing" in its turn, and confirm the panel still says
  `No test report yet`. That test is the point of this panel

> **Ruling.** `3 / 3 passing · 100%` is the most dangerous element in the mock:
> a green bar reading 100% over an unmeasured fact. The panel ships as the
> unknown copy. It is the strongest argument in this spec for the report kind in
> §13.

### D6 — `TEAM`
- cite: `Singularity.png → inspector → TEAM → "Keystone Lead idle / Builder
  Coding agent ● working / Reviewer Review agent ● reviewing"`
- replaces: `CodingSessionExecutionRail.tsx:257-300` (`ExecutionCard`) and
  `:302-320` (footer)
- reads: **W8** + **W1** + **W13**
- copy: row = `Keystone` / `Lead` / state word from **W1**; secondary line
  `Codex · gpt-5.6-sol`; per-seat `Model` / `Runtime` / `Generation` /
  `Last activity` facts kept verbatim from `ExecutionDetail`
  (`:235-248`); empty activity kept verbatim: `No attributable activity yet.`;
  spawn note kept verbatim: *"Spawned agents will appear here when the session
  publishes signed spawn relationships."*
- walk: **seen**, `03` — and this capture is a bug: the rail says `Idle` for a
  seat the strip beside it calls `live`. See §15(b). The lane deletes
  `executionStatus` (`CodingSessionExecutionRail.tsx:398`) and threads the
  demoted resolver in; the footer's `All idle` (`:316`) goes with it, replaced
  by `1 working · 1 idle` computed from the same demoted status

### D7 — `Context` tab
- cite: `Singularity.png → inspector → "Context" tab (unselected in the mock)`
- replaces: `CodingSessionHeader.tsx:299-346` provenance popover — moved here
- reads: **W6** (founder), **W12** (context load), 44223 target/runtime
- copy: `Founded by <name>` (never a bare pubkey; the truncated key stays as the
  monospace secondary, existing `Verified source` row);
  `Signed projection <generation label>`; `Context <used>/<window> (14%)` and,
  when no window is known, `<used> tokens (window unknown)` — the exact
  rendering `bee sessions status` uses (`crew_cmds.rs:916-923`); when nothing
  has reported → `—` with the hover `no usage reported`
- walk: open the tab on a Claude seat (emits `context_window_updated`) and on a
  seat that does not; confirm the second says `window unknown` and never a
  percentage

### D8 — Nothing-selected / collapsed
- cite: `Singularity.png → inspector` is drawn open with nothing selected
- replaces: `CodingSessionSurfaceHost.tsx` chrome — kept whole (width, sheet,
  close), only its contents change
- reads: n/a
- copy: the inspector's default is the overview above; collapsed it is a single
  right-edge control with `aria-label` `Show session state`
- walk: **seen**, `05` (resized) and `07` (narrow → sheet)

---

## 7. Region E — Live activity bar

**Mock:** `Singularity.png → live activity bar → "B Builder · implementing tests
· 3m 18s · 6 tool calls · R Reviewer · reviewing · K Keystone · idle ·
[Follow live] [Stop all]"`.

Replaces `CodingSessionActiveWorkDock.tsx:34-130` — a `max-h-[min(48vh,28rem)]`
card. **Seen**, `01`/`02`/`04`: it covers the lifecycle row and a third of the
stream. Target height 44px, one row, horizontally scrollable.

### E1 — Working-seat segment
- cite: `… → "● Builder · implementing tests · 3m 18s"`
- replaces: `CodingSessionActiveWorkDock.tsx:94-124` (agent chips) and `:132-161`
  (`ActivePlan`)
- reads: **W1** (that it is working) + **W10** (the phrase)
- copy: `Builder · implementing tests`; with no plan snapshot, the fallback is
  the shorter truth `Builder · working` — **not** today's sentence
  *"<label> is working. No signed plan has been published for this turn."*,
  which is right but is a paragraph in a 44px bar (it moves to the inspector's
  PLAN empty state, D2)
- walk: **seen**, `01`

### E2 — Elapsed `3m 18s`
- cite: `Singularity.png → live activity bar → "3m 18s"`
- replaces: nothing — **new** (the current dock shows no elapsed time)
- reads: **W2** — `turn_started` for the open turn; failing that, the first
  44225 item of the open turn
- copy: `3m 18s`; unknown → the segment omits the clause entirely (never `0s`)
- walk: seed a `turn_started` without transcript items and confirm the clock
  runs; remove it and confirm the clause disappears rather than reading zero

### E3 — `6 tool calls`
- cite: `Singularity.png → live activity bar → "6 tool calls"`
- replaces: nothing — **new**
- copy: see the two stages below; neither stage ever renders `0`
- reads: **W12**, in two stages, and they must not be confused:
  - *while the turn is open* — a count of 44225 tool items in that turn. Copy:
    `6 tools this turn`
  - *once the turn closes* — the signed `usage.toolCalls` on the `result` item.
    Copy: `6 tool calls`. Where the two disagree the signed one wins
  - neither → the clause is omitted, never `0`
- walk: assert the open-turn count matches the expanded bundle's row count, then
  close the turn with a `result` item carrying `toolCalls: 9` and assert the
  segment switches to `9 tool calls`

> **Ruling.** The mock shows a live tool count; `usage` only exists on the
> terminal item. Rather than drop the number or invent it, the bar says which
> kind of number it is holding. `this turn` is doing real work in that string.

### E4 — Non-working seats in the bar: **dropped**
- The mock shows `Reviewer · reviewing` and `Keystone · idle` in the bar; both
  are already in the participant status bar two rows up, at greater weight.
  Presence without noise means one home per fact.

### E5 — `Follow live`
- cite: `Singularity.png → live activity bar → "Follow live"`
- replaces: nothing — **new** (today the narrative auto-scrolls on focus,
  `CodingSessionUmbrellaWorkspace.tsx:596`)
- reads: n/a — client scroll state
- copy: `Follow live` / when the viewer has scrolled up, `New activity ↓`
- walk: scroll up mid-turn, confirm the viewport does not jump and the control
  changes

### E6 — `Stop all` moved, `Pause` **dropped**
- `Stop all` moves to A7, keeping its count and its sentence.
- **`Pause`: dropped.** Named in the design doc's §5 and drawn in the ASCII
  layout. There is no pause verb on the wire — the commands are turn, interrupt,
  stop. A control that cannot guarantee the thing does not get the label. See
  §12.

---

## 8. Region F — Composer

**Mock:** `Singularity.png → composer → "Message Singularity…" + [@ Singularity]
[Attach] [Add context] [Mode: Live ▾] [↑]`.

### F1 — Addressee
- cite: `… → "@ Singularity"`
- replaces: `CodingSessionUmbrellaComposer.tsx:347-475`
  (`CodingSessionParticipantPicker`) — kept whole
- reads: **W8** for the list; the lane entry needs `sessionRef` and collapsed
  history (`umbrellaHasCollapsedHistory`)
- copy: existing verbatim — trigger `Send to Codex · gpt-5.6-sol` (**seen**,
  `01`); popover header `Send to`; the lane row `Session` /
  `Everyone in this session`; footer *"A leading @name also routes an agent
  prompt."*; mention hints verbatim, all three:
  *"Sending to <label> — @<raw> is removed from the prompt."*,
  *"@<raw> fits <a> and <b>, so it stays plain text. Pick one above."*,
  *"@<raw> is <label>, but <reason>, so it stays plain text."*
- copy, placeholder: `Message <addressee>…` — resolved, never the generic
  `Message Singularity…` when a specific seat is selected
- walk: **seen**, `01`/`08` (narrow). Type `@cod` and read the hint

### F2 — Observe-never-steer
- cite: mock has no equivalent — **new to the mock, existing in code**
- replaces: `CodingSessionUmbrellaComposer.tsx:210-219` (gated composer)
- reads: **W4**
- copy: existing verbatim, all four —
  `View only — ask the session owner for collaborator access`;
  *"Only the session founder can prompt executions in this version. The session
  lane stays open to every member."*;
  *"Session authority could not be resolved from its genesis. Controls are
  disabled until authority is available."*;
  *"Session authority is loading. Controls remain disabled until your identity
  is available."*
  The `Can control` chip stays as the positive case (**seen**, `01`)
- walk: open a governed umbrella as a non-founder; the editor is replaced by the
  reason, and the Session lane still accepts a message

### F3 — Unreachable target
- cite: mock has no equivalent — **existing surface, kept**
- replaces: `CodingSessionComposerSurface.tsx:360` — kept verbatim
- reads: **W1**
- copy: existing verbatim — *"No provider is answering for this execution — <detail>.
  Add a provider to the session to continue the work."*
  (`CodingSessionComposerSurface.tsx:360`), input disabled
- walk: `coding-session-reachability` spec test 1 — **seen** (assertion-level)

### F4 — Untargeted execution
- cite: mock has no equivalent — **existing surface, kept**
- replaces: `CodingSessionUmbrellaComposer.tsx:299-308` — kept verbatim
- reads: 44223 `commandTarget` (absent)
- copy: existing verbatim — *"This execution has not published a governed
  command target yet."*
- walk: seed a metadata event with no `cs-target` and open the composer on it

### F5 — `Attach`, `Add context`, `Mode: Live ▾`: **dropped** — see §12

---

## 9. Region G — Brief / Live / Trace

**Mock:** `Singularity.png → mode tabs → "Brief | Live | Trace"`, Live
underlined.

### G1 — Density selector
- cite: `Singularity.png → mode tabs`
- replaces: nothing — **new**
- reads: nothing. **A density mode reads no wire row; it is a lens over the
  items already rendered.** It never fetches, never hides a row it cannot count,
  and never changes what is sent
- copy:
  - tabs `Brief` `Live` `Trace`; `aria-label` on the group `Reading density`
  - Brief hides progress entries and execution bundles. It must say so, in the
    stream, at the position things were removed:
    `Brief is hiding 14 execution bundles and 6 progress updates. Switch to Live
    to read them.`
    Singular: `Brief is hiding 1 execution bundle.`
  - Trace expands every bundle and adds the per-item metadata already available
    (timestamps, durations, `costUsd` when the item carries it, `usage` when the
    `result` item carries it). No new facts
  - The lifecycle chip (A3) and the density tabs never share a word: the chip
    says `Running`, the tab says `Live`. This is the design doc's own warning
    (§Brief/Live/Trace) and it is the reason the chip is not labelled `Live`
- **The rule Brief cannot break:** every C5 attention entry renders full-size in
  all three modes. Brief is a lens, not a filter on consequence
- walk: switch to Brief on the surface-host fixture; count the hidden-items line
  against the bundles in Live; seed a 44240 `blocker` and confirm it is still
  full-size in Brief

---

## 10. Region H — MISSION card

Per `t3-workflow-card-header.png` and `t3-workflow-card-expanded.png`. A mission
is a script somebody can read, not a status somebody reports.

The card renders in the stream at the position of the earliest 44240 `plan`
entry that opens a chain. **No lead is assumed:** the card belongs to whichever
identity signed the chain, and multiple chains render multiple cards.

### H1 — Card header
- cite: `t3-workflow-card-header.png → "● SWAT-TEAM-LAUNCH-I… {} script 3/3
  settled ⌄"`
- replaces: nothing — **new**
- reads: **W3** (the chain), **W1** (the leading dot)
- copy: `● <mission name> · Keystone` — the author is named on the header,
  because everything inside is that author's claim. Settled counter
  `3/3 settled`; when the chain has no closing entries `0/3 settled`; no chain →
  the card does not render at all (never an empty card)
- walk: publish three `plan` entries from a seat and read the header

### H2 — `{} brief` chip
- cite: `t3-workflow-card-expanded.png → "{} script" chip → opens
  "swat-team-launch-in-project-wf_e98fd842… ✕" with the script body`
- replaces: nothing — **new**
- reads: **W16**
- copy: chip `{} brief`; opened, a monospace panel titled with the seat's target
  and dismissable; when the seat's opening turn is not in this client's store →
  `no brief on the wire` (the chip renders disabled with that as its title)
- walk: open a hired seat's card and read the exact dispatch text the lead sent

> This is the T3 idea worth stealing whole: the instruction is *viewable in
> place*. Today the only way to read what a seat was told is to scroll to the
> top of its transcript.

### H3 — Phase pills
- cite: `t3-workflow-card-expanded.png → "✓ Fix ● › ✓ Gate ● › ✓ Land ●"`
- replaces: nothing — **new**
- reads: **W3** — one pill per `plan` entry in the chain, in publication order;
  a pill is settled when a `milestone` entry `supersedes` it; a pill is blocked
  when a `blocker` entry supersedes it
- copy: `Fix` unsettled; `✓ Fix` settled; `Blocked · Fix` with the blocker's
  first line beneath, in the destructive tone. No chain → `no pulse yet`
- walk: `bee pulse update --kind plan --text Fix`; then
  `--kind milestone --supersedes <id>`; confirm the pill checks. **not
  reached** — no Pulse fixture in the mock bridge; lane 2 adds one

### H4 — Seat rows `opus-5[1m] · 193k tok · 99 tools`
- cite: `t3-workflow-card-expanded.png → "● fix ✓ / ▸ StructuredOutput /
  opus-5[1m] · 193k tok · 99 tools"`
- replaces: nothing — **new**
- reads: **W13** (model), **W12** (tokens, tools), **W1** (the dot)
- copy: `opus-5[1m] · 193k tok · 99 tools`; each clause independently omitted
  when unreported, with the row never collapsing to zero:
  `model unknown`, `tokens not reported`, `tools not reported`. Token formatting
  matches `bee sessions status`: raw count, `k`/`M` abbreviation only above
  1 000
- walk: read one seat's row against `bee sessions status --format json` for the
  same target; the `usedTokens` must match

### H5 — Footer `1 working · 80 settled · Σ 7.9M tok`
- cite: `t3-workflow-card-expanded.png → footer "● 1 working  80 settled
  Σ 7.9M tok"`
- replaces: `CodingSessionExecutionRail.tsx:302-320` footer (`All idle`)
- reads: **W1**, **W2**, **W12**
- copy: `1 working · 80 settled · Σ 7.9M tok`. **The sum is only a sum when
  every seat reported.** Otherwise: `Σ 7.9M tok (3 of 5 seats reported)`. When
  no seat reported: `Σ tokens not reported`
- walk: seed three seats, two with `usage`, and confirm the footer discloses the
  partial

> **Ruling.** A total with missing terms presented as a total is the quietest
> lie a dashboard tells. The parenthetical is not decoration; it is the whole
> honesty of the number.

---

## 11. Every number in the mock, sourced or unknown

| Mock element | Source | Verdict |
|---|---|---|
| `Changes 12` | W11 | sourced (relabelled `Observed changes`) |
| `5:05 PM` … `5:17 PM` | W7 item timestamps | sourced |
| `11 execution events`, `9 execution events` | W7 tool-item count | sourced |
| `Terminal 4 · Read 2 · Edit 3 · Search 2` | W7 + classifier | sourced |
| `Dispatch extraction · 3 files` | W7 handoff quote | sourced |
| `+84` `−8` | W11 | sourced |
| `~12` (modified) | — | **dropped**, §12 |
| the three-colour proportion bar | — | **dropped**, §12 |
| `FILES · 4 modified` | W11 | sourced |
| `+42 −8`, `+28 −0`, `+36 −0`, `~0 −4` | W11 per file | sourced; a null count suppresses the pair rather than printing `0` |
| `TESTS 3 / 3 passing`, `100%`, the green bar | — | **unknown** → `No test report yet` (D5) |
| PLAN's 5 rows and their ✓/→/○ | W10 | sourced, **owner named** (D2) |
| `3m 18s` | W2 | sourced; omitted when unknown |
| `6 tool calls` | W12 | sourced, in two stages (E3) |
| `Running` | W1 | sourced, **after demotion** (A3) |
| `Keystone · Lead`, `Builder · Working`, `Reviewer · Reviewing` | W8 + W1 | role sourced; the state words are replaced by the W1 vocabulary (§1) |
| `2 members · 1 active` (doc layout only) | W8 + W1 | **dropped** as an aggregate (B2) |

---

## 12. Dropped from the mock

| Element | Reason |
|---|---|
| `Singularity` pill beside the name (A6) | The sidebar section already says it. |
| `2 executions` in the header context line (A5) | The participant bar counts itself, with names. Survives in the provenance popover. |
| Aggregate `2 agents · 1 working` chip (B2) | A summary of the row beside it. Its focus function survives as B3. |
| `~12` modified column + proportion bar (D3) | `~` is a per-file modified count drawn as if it were a line count, and the bar implies a proportion of a whole diff nothing measured. |
| Non-working seats in the live bar (E4) | Already in the participant bar at greater weight. |
| `Pause` (E6) | No pause verb exists on the wire. A control does not claim what it cannot enforce. |
| `Attach`, `Add context` (F5) | No signed path in this batch; adding them would ship two controls with no contract. Revisit with the context work. |
| `Mode: Live ▾` in the composer (F5) | Duplicates the Brief/Live/Trace tabs. One control per job. |
| Per-entry type words without a Pulse (C4) | Client classification of prose. Types come from 44240 or the entry has none. |
| `Keystone.png` as a rendered avatar | The mock draws letter monograms (`K`, `B`, `R`), not this image. The file is the identity's kind-0 `picture`; when the profile carries one it renders, otherwise the monogram. Not a new surface. |

---

## 13. Requires a new wire artifact

Written in the words `wire-sources-for-surfaces` requires. Each **needs Brian's
sign-off**.

**(1) Tests and changes — a structured lane report.**
*Tests: no signed source today* — what would have to exist: a report event
(kind TBD, or a `report` transcript item subtype) carrying the builder's
`write-report` fields as data, not prose: `branch`, `headSha`, `filesTouched[]`,
`tests[] {name, count, exitCode}`, `redBeforeGreen`, `deviations[]`,
`residuals[]`. Surface shows: `No test report yet`. Until then D5 never renders a
number, and D3 renders only what tool items observed.

**(2) The accepted plan — the lead's acceptance steps as a published artifact.**
*Accepted plan: no signed source today* — what would have to exist: the lead
publishing its brief's `Acceptance:` block as a 44240 `plan` entry per step (or
a dedicated kind), so a step can be checked against a receipt rather than
against a sentence. Surface shows: `No accepted plan published`. Until then D2
renders the *seat's own* plan with the seat's name on it, and H3's pills come
from whatever Pulse the author chose to publish.

**(3) Not required after all — per-turn usage.** My brief listed tokens and tool
calls here. They exist (W12, ledger 89(b)). Removed from this list; H4, H5 and
D7 are specified against the real block.

---

## 14. No surface, by decision

Each **needs Brian's sign-off**.

1. **Mobile: no surface, by decision** — desktop first, per D16 and the brief.
   The Flutter app renders no coding session today; adding one behind this spec
   would be a second implementation of an unstable surface.
2. **Web: no surface, by decision** — the web client is the repo browser; no
   session route exists to extend.
3. **CLI: no new surface, by decision** — `bee sessions status` already prints
   seat, liveness, open turn, turn budget and context; the facts this spec adds
   (W3 chains, W16 briefs) are readable with `bee pulse list` and
   `bee sessions inbox`. If the Mission card proves itself, `bee sessions
   mission` is the follow-up, not this batch.
4. **A "hire a seat" button: no surface, by decision** — D14 says the lead hires
   with `bee sessions hire` after hearing the mission. Putting a hire button in
   the Singularity header would offer the operator a second, weaker path to the
   same act. `Add provider…` (A7) stays as the manual attach.
5. **Pause: no surface, by decision** — no wire verb (E6).
6. **Per-participant permanent columns: no surface, by decision** — the design
   doc argues it and I agree; one chronology, one lens.

---

## 15. Anomalies found while driving

Handed to the lead as findings, not fixed here.

**(a) My brief's premise on usage is out of date.** Tokens, tool calls and
context window are on the wire since ledger 89(b) (`coding_session_payload.rs:1196`,
`crew_cmds.rs:884`). The MISSION card's numbers are sourceable today. This
*shrinks* the "requires new wire artifact" list to two items.

**(b) Two voices for one liveness fact — confirmed live, `03-agents-open.png`.**
In one screenshot the disposition strip says `Claude Code · sonnet · live`, the
header says `1 working`, and the Agents rail says `Idle` for that same
execution. Cause: `CodingSessionExecutionRail.tsx:398` (`executionStatus`) reads
the raw 44223 status with no reachability demotion and no transcript-derived
open-turn test, while the strip uses `deriveCodingSessionWorkspaceStatus`. The
rail's footer `All idle` (`:316`) inherits it. Severity: this is the ledger-77(c)
class — a stock component painting a comfortable word over a fact the surface
beside it already knows. Fixed by D6 in lane 2.

**(c) The umbrella lifecycle status is never demoted.** `deriveUmbrellaStatus`
(`codingSessionUmbrellaModel.ts:486`) folds raw statuses, so an umbrella whose
providers are all unreachable can paint `Running` in the header while every one
of its chips reads `No provider answering`. Not reproduced live (the fixture has
a live lease); found by reading, and A3 specifies the fix.

**(d) The active-work card occludes the stream.** Visible in `01`, `02` and `04`:
the card covers the `Codex · gpt-5.6-sol joined this session` lifecycle row.
Cosmetic today, but it is the reason a person cannot see a seat arrive. E1
replaces the card.

---

## 16. Lane briefs

Two lanes, no shared files. Neither lane touches the wire, the relay, the CLI,
mobile or web. Neither lane changes `codingSessionWorkspaceModel.ts` — both read
it.

### Lane 1 — Stream and strip

**Owns, exclusive:**
```
desktop/src/features/coding-sessions/ui/CodingSessionHeader.tsx
desktop/src/features/coding-sessions/ui/CodingSessionParticipantBar.tsx      (new)
desktop/src/features/coding-sessions/ui/CodingSessionLiveActivityBar.tsx     (new)
desktop/src/features/coding-sessions/ui/CodingSessionActiveWorkDock.tsx      (deleted)
desktop/src/features/coding-sessions/ui/CodingSessionAgentFocus.tsx
desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaTurnBlock.tsx
desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx   (the wiring file)
desktop/src/features/coding-sessions/lib/codingSessionUmbrellaModel.ts
desktop/src/features/coding-sessions/lib/codingSessionStreamDensity.ts       (new)
desktop/tests/e2e/singularity-stream.spec.ts                                 (new)
```
Lane 1 owns `CodingSessionUmbrellaWorkspace.tsx` and therefore wires lane 2's
components. Lane 2 exports components with props; it never edits this file.

**Delivers:** A3 (demoted lifecycle chip), A4–A7, B1–B4, C1–C7, E1–E6, G.

**Red first, one per claimed state:**
- `an unreachable umbrella never paints Running in the header` — every execution
  leaseless; assert the chip reads `No provider answering`, not `Running`
- `a seat chip reads agent and role, never a pubkey` — seed `agentRef` + `role`;
  assert `Keystone · Lead`; assert no 64-hex substring in the bar
- `a chip with no profile falls back to role and runtime` — `agentRef` set, no
  kind-0; assert `designer · Codex · gpt-5.6-sol`
- `an execution with no transcript says no turn observed` — assert the chip
  title, not `0s`
- `a bundle's count equals the rows it reveals` — expand and count
- `one execution event is singular`
- `a blocker renders full-size in Brief` — seed 44240 `blocker`, switch to
  Brief, assert full-size
- `Brief names what it is hiding` — assert `Brief is hiding 3 execution bundles.`
- `turn_degraded says steer was degraded` — seed the receipt, assert the
  sentence verbatim
- `the live bar omits the elapsed clause when no turn_started exists` — assert
  no `0s`
- `an open turn counts tools this turn and a closed one counts tool calls` —
  both strings, one fixture

**Done when:** on the surface-host fixture at 1440×900, a person can name every
seat, its state, and what the working one is doing without scrolling, and the
stream's first lifecycle row is unoccluded. Captures re-run and hashes distinct.

### Lane 2 — Inspector and mission

**Owns, exclusive:**
```
desktop/src/features/coding-sessions/ui/CodingSessionInspector.tsx           (new)
desktop/src/features/coding-sessions/ui/CodingSessionExecutionRail.tsx
desktop/src/features/coding-sessions/ui/CodingSessionChangesRail.tsx
desktop/src/features/coding-sessions/ui/CodingSessionTaskRail.tsx
desktop/src/features/coding-sessions/ui/CodingSessionSurfaceHost.tsx
desktop/src/features/coding-sessions/ui/CodingSessionMissionCard.tsx         (new)
desktop/src/features/coding-sessions/lib/codingSessionMissionModel.ts        (new)
desktop/src/features/coding-sessions/lib/codingSessionUsage.ts               (new)
desktop/tests/e2e/singularity-inspector.spec.ts                              (new)
```

**Delivers:** D1–D8, H1–H5.

**Red first, one per claimed state:**
- `the tests panel says No test report yet even when a turn claims 3/3 passing`
  — seed an assistant message containing "3 / 3 tests passing"; assert the panel
  never renders a count. **This is the lane's headline test**
- `the plan panel names the seat whose plan it is` — assert `PLAN · Keystone`
- `no plan snapshot renders No plan published` — verbatim, with its description
- `the agents rail and the participant bar agree on liveness` — one leaseless
  execution; assert the rail does not say `Idle` while the bar says
  `No provider answering`
- `a null line count suppresses the total` — one file with null additions;
  assert `4 files · line counts not reported` and no `+0`
- `context with no window never shows a percent` — assert
  `137498 tokens (window unknown)`
- `a mission card without a pulse chain does not render`
- `a settled phase needs a milestone that supersedes its plan` — plan alone →
  unsettled; add the milestone → `✓ Fix`
- `a blocked phase reads Blocked and carries the blocker text`
- `the brief chip disabled when the opening turn is absent` — assert
  `no brief on the wire`
- `a partial token sum discloses how many seats reported` — assert
  `Σ 7.9M tok (2 of 3 seats reported)`
- `no seat reported renders Σ tokens not reported`

**Done when:** with the inspector open on the surface-host fixture, a person can
answer *what is the goal, whose plan is running, what changed, what do we know
about tests, and who is on the team* without opening the transcript — and the
tests answer is the honest one.

---

## 17. Left out, deliberately

- **The Context tab's non-usage contents** (branch, environment, permissions,
  connected resources) from the design doc's §4. Branch is not on the wire for an
  execution today; the rest would be invented. D7 ships founder, projection and
  context load only.
- **Filtering and search across bundles** (design doc §"Stream behavior"). Real,
  and a third lane. Not specified here.
- **Human-teammate Singularities** (design doc §"Human-team"). Nothing on the
  wire seats a human as a participant yet; the participant bar's model is
  role-shaped and will take one when W8 does.
- **Auto-follow supersession semantics** (design doc §"Editing and
  supersession"). Today a live turn's items already stream in place; correcting
  a durable item is a wire question, not a surface one.
- **Accessibility beyond the two rules stated** (reduced motion in B1, polite
  announcements on C5). The lanes inherit the repo's existing rules — rem tokens
  only, never colour alone, disclosure rows keyboard-reachable.
