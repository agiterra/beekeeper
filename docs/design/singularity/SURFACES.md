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

**Captures:** `docs/design/singularity/walk-2026-08-29/0{1..9}-*.png`, nine
states, distinct hashes. Those committed files are **lane E's copies** of the
same spec's output, not the run I read — Texas re-ran
`coding-session-surface-host-screenshots` against the same fixtures during the
walk and copied his out in time. Every string this spec quotes from `01`–`09`
reads the same in both sets; the per-run difference is the provider key in the
turn-block byline (`17888150…952d` in mine, `e644a2d2…4399` in the committed
set), which is generated per run — and is the very element **C1a** evicts from
that byline.

**Why not my own copies:** captures written under `test-results/` are **not
durable**. Playwright empties that directory at the start of every run, so any
spec run in a desktop worktree destroys them, and a citation pointing there
resolves to an empty directory that cannot be told apart from evidence that
never existed. The build lanes will run specs in that worktree. Cite the
committed path, never `test-results/`.

States I could not reach are written **not reached**, with why.

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
| W1 | Liveness | **Signed 44223 `status`, demoted by the ephemeral lease — never the transcript.** The transcript open-turn test may only *narrow* a live seat to "working right now"; it may never promote (§15(b), lane 0) | Five words: `live` · **`waiting for you`** / **`waiting for an operator`** · `idle` · `released` · **`No provider answering`** + `last reported Idle 2h ago`. See §2a |
| W2 | Turn stages | 44224 `turn_queued`/`turn_started`/`turn_degraded`/`turn_dropped`/`turn_refused`/`interrupt_delivered` | `no receipt yet` |
| W3 | Dispositions | 44240 Pulse, `pu-type` ∈ `plan`\|`milestone`\|`note`\|`handoff`\|`blocker`, chained by `supersedes` | `no pulse yet` — rendered as a claim **by its author**, never as a verdict |
| W4 | Authority | 44228 `grant-operator` + roster fold | `View only — ask the session owner for collaborator access` |
| W5 | Goal | 44227, `d=sessionRef` | `No goal set` |
| W6 | Founder | 44226 genesis signer; identity is the genesis **event id** | never unknown |
| W7 | Story | 44225 transcript items | `no turn observed` |
| W8 | Seats | 44223 `agentRef` + `role`; display name from kind-0 | `unseated`; never a pubkey |
| W9 | Hires | 44221 `session.hire` + 44224 receipt (`HIRE_MODEL_NOT_OFFERED`, `HIRE_STALE`) | `hire not answered` |
| W10 | Seat plan | 44225 `plan` item / `update_plan`/`todo` tool item (`codingSessionTaskModel.ts:58`) — **the seat's own plan, not an accepted one** | `No plan published` |
| W11 | Observed changes | 44225 tool items with file edits (`deriveCodingSessionChangedFiles`). **Three states, not two** — no edit items; edit items that name files; edit items that name nothing (`toolKind:"edit"` with empty `input`) | zero edits → `No observed changes yet`; **edits with no nameable file → `16 edits observed · files not reported`** |
| W12 | **Tokens, tool calls, context** | 44225 terminal `result` item, `usage` block — `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `toolCalls`, `contextWindow` (`coding_session_payload.rs:1196`); occupancy from the `context_window_updated` item (`coding_session_payload.rs:1268`) | `tokens not reported` / `tools not reported` / `window unknown` |
| W13 | Model / runtime | 44223 `model`, `runtime` | `model unknown` |
| W14 | **Tests** | **none today** | `No test report yet` |
| W15 | **Accepted plan** (the lead's acceptance steps) | **none today** — the brief's `Acceptance:` line is prose | `No accepted plan published` |
| W16 | Mission brief | the 44220 turn command the seat was opened with, echoed as its first 44225 item | `no brief on the wire` |
| W17 | **Pack** — which persona files this seat was staged from | 44223 `packRef` (`repo`, resolved `sha`, `role`, `path`), pointing at the project's kind:30624 pack source (`docs/nips/NIP-PK.md`), or `repo: "app:shipped"` with the app version as `sha` for the build's bundled packs. The **seat's** role picks the pack; the actor's home role is never consulted | key absent → **`no pack staged`** — the seat ran the session checkout's own `personas/roles/<role>/`. Never a default pack name, and never blank. The three sources are named `packs repository` · `session checkout` · `shipped defaults`, spelled once in `beekeeper_core::project_pack_source` |

**W12 corrects my brief.** The brief lists tokens and tool calls under "requires
new wire artifact". They landed in ledger item 89(b) and are on `main` at
`19ad1b97`: the `usage` block rides the 44225 `result` item, and
`bee sessions status` already prints a context column from it
(`crates/beekeeper-cli/src/commands/sessions/crew_cmds.rs:884-933`). The T3
reference's `opus-5[1m] · 193k tok · 99 tools` is **sourceable today**. Nothing
in this spec fakes it from transcript length. See §15(a).

Three rules the lanes inherit: absence ≠ zero; one row = one voice (two panels
showing W1 use the same word); a claim (W3, W10, W16) is rendered with its
author's name attached, an observation (W1, W2, W7, W12) is not.

## 2a. W1's five words

*(Added 2026-08-29, lane D3. `waiting_for_input` is a signed 44223 status that
`codingSessionWorkspaceModel.ts` never maps — it falls through to
`{kind:"idle"}` at `:208`, so a seat blocked on a person reads `idle`. The rail
footer is the only surface that shows it today, from a second read of the raw
status, and folding that footer onto W1 would have deleted it. The wire already
treats it as its own tier in `deriveUmbrellaStatus`
(`codingSessionUmbrellaModel.ts:497`), so the seat-level vocabulary is the odd
one out; this removes an inconsistency rather than adding one.)*

| kind | signed source | word |
|---|---|---|
| `working` | `running` / `starting` | `live` |
| **`waiting`** | **`waiting_for_input`** | **`waiting for you`** / **`waiting for an operator`** |
| `idle` | `idle`, `completed`, anything unmapped | `idle` |
| `ended` | `stopped` | `released` |
| `unknown` | unreachable, `disconnected`, `failed`, absent | `No provider answering` / `Disconnected` / `Needs attention` / `Status unknown` |

**Precedence — reachability outranks stage.** An unreachable seat whose signed
status is `waiting_for_input` reads `No provider answering`, never a waiting
word. `demoteUnreachable` must demote `waiting` exactly as it demotes `working`
(`codingSessionWorkspaceModel.ts:241-265`, which returns early only for `ended`
and `unknown`). Telling an operator that a dead seat is waiting on them invites
them to type into something nobody will read — a worse lie than the collapse
this word fixes.

**The transcript heuristic can never produce the waiting word.** It narrows
`live` to "working right now" and does nothing else. Unchanged from §15(b).

**The umbrella fold is untouched.** `deriveUmbrellaStatus` keeps its tier order
(running outranks waiting, `:497`), so a team with one seat running and one
waiting reads `Running`. A3 shows a waiting word only when the fold itself
returns `waiting_for_input`.

**Why two strings, and where the choice is made.** The word has to name who is
waited on — that is the actionable half, and it is the only thing that makes
this state worth a word rather than a shade of `idle`. But `waiting for you` is
false for a reader who cannot answer: **W4** gates prompting, and a view-only
observer told "waiting for you" will try, and be refused by the composer's own
`View only — ask the session owner for collaborator access` (**F2**). So:

- viewer holds authority (`canPromptExecutions`) → `waiting for you`
- viewer does not → `waiting for an operator`

The choice is made **once**, in the shared word mapper
(`codingSessionDispositionWord`, `codingSessionUmbrellaModel.ts:589-601`, which
gains the viewer's steer flag), not at each render site. Lane 0's status
function stays viewer-independent and returns the `kind`; only the string
depends on authority. One function, one place, two strings — the one-voice rule
holds.

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
- copy: `Running` · **`Waiting for you`** / **`Waiting for an operator`** ·
  `Idle` · `Ended` · `Needs attention` · `No provider answering`. Detail clause
  preserved verbatim: `last reported Idle 2h ago`
  (`codingSessionWorkspaceStatusDetail`). The waiting label appears only when
  the umbrella fold itself returns `waiting_for_input` — running outranks
  waiting in `deriveUmbrellaStatus` (`codingSessionUmbrellaModel.ts:497`) and
  that tier order is not touched, so one seat running beside one waiting still
  reads `Running` (§2a)
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
  - line 2, one of: `live` · `waiting for you` / `waiting for an operator`
    (§2a) · `idle` · `released` · `No provider answering` ·
    `Needs attention` · `Disconnected`
  - line 3, only while `live` and only when a plan snapshot exists: the
    in-progress task text, quoted from the seat, ≤ 48 chars, e.g.
    `reviewing the dispatch change`. Absent → no third line (never `working…`)
  - line 4, the pack line (**W17**, L23): `pack <role>@<sha8>` when the seat's
    44223 names a packs repository, `pack <role> · shipped defaults <version>`
    when it names `app:shipped`, and `no pack staged` when the key is absent —
    because "which prompt did that agent actually run" is a question a person
    must be able to answer from the chip, and a build's bundled packs are an
    answer, not a blank
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

### C1a — Turn-block byline
*(Added 2026-08-29 — walk finding 4. B1 said "never a pubkey" but claimed only
the strip, so the one surface that actually prints a key went unspecified.)*
- cite: `Singularity.png → main stream → the "Builder" byline above each entry`
  (the mock prints a name and no key)
- replaces: `CodingSessionUmbrellaTurnBlock.tsx:154-159` (the monospace
  `truncatePubkey(block.signerPubkey)` beside the label) and `:128-131` (the
  screen-reader line *"Response from Lead, signer 8b830553…fbc0, generation
  1."*)
- reads: **W8** — the seat's own identity from `agentRef`, resolved to a display
  name through kind-0. Never `signerPubkey`
- copy: `Keystone · Lead` then `generation 1`. Unseated execution →
  `Codex · gpt-5.6-sol · generation 1`. Seated but no profile read → role and
  runtime. Screen-reader line: `Response from Keystone, Lead, generation 1.`
  The provider key moves to D7's popover under its existing honest label
  `Verified source`, where it is provenance rather than identity
- walk: open the umbrella on real events; read the byline of each of three
  seats. Walk finding 4: today all three read the same `8b830553…fbc0`, the
  provider key that signs for every seat, while the Agents rail resolves
  `Lead`, `Designer`, `Poker` correctly from the same records

> **Ruling.** The word "signer" is true and the key really is the signer — but
> it sits in the identity slot, it is byte-identical on every seat in the
> session, and it is the one thing on the row a person would use to tell two
> seats apart. A reader learns nothing and believes they learned an identity.
> Ledger 77(c), still shipped. Independently confirmable without the render
> harness: `bee sessions status` shows all seats sharing one `signer` while
> their `actor` values differ.

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
- copy, **three states**:
  - zero edit items observed → existing verbatim: `No observed changes yet` /
    *"File edits observed in this session's transcript will collect here."*
  - edit items observed, none nameable → `16 edits observed · files not reported`
    / *"The provider published these edits without a path or a diff, so
    Beekeeper cannot name the files. Nothing was hidden — nothing was sent."*
  - files resolved → existing rows verbatim; per-row `View` / `Hide`; a file
    with no diff renders its name with no counts, never `+0 −0`
- walk: **seen**, `04` (files resolved). For the middle state, walk finding 3 on
  real relay events: 16 of 188 `tool_call` items carry `toolKind:"edit"`, every
  one with an empty `input`, and the tab renders `No observed changes yet`
- **A4's count follows the same rule**: the header tab counts *nameable files*,
  so in the middle state it shows no count and the panel carries the sentence.
  A tab reading `Observed changes 0` beside 16 observed edits would be the same
  lie in a smaller font

> **Ruling.** Presence rendered as absence is the mirror of the rule this spec
> opens with. `No observed changes yet` is reserved for **zero** observed edits;
> it may never stand in for "we saw sixteen and can name none." The provider
> stripping edit payloads before they reach the wire is a wire-side bug on the
> ledger — this surface reports what arrived and does not reconstruct paths.

### D5 — `TESTS`
- cite: `Singularity.png → inspector → TESTS → "3 / 3 passing" + a full green
  bar + "100%"`
- replaces: nothing — **new**
- reads: **W14 — no signed source today.**
- copy, always, until W15/W14 exist, in the elision shape of §10a — a refusal
  that names what it is refusing:
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
- walk: **seen**, `03` — and this capture is a bug in both directions: the rail
  says `Idle` for a seat the strip calls `live`, and on real events with stale
  leases the rail says `Working` for a seat the strip calls `no provider
  answering` (walk finding 1). See §15(b). The lane deletes `executionStatus`
  (`CodingSessionExecutionRail.tsx:398`) and calls lane 0's single W1 function;
  the footer's `All idle` (`:316`) goes with it, replaced by
  `1 working · 1 waiting for you · 1 idle` computed from that same function —
  **including the waiting count**, which today is a second read of the raw
  status (`:310-311`) and is the reason W1 has a fifth word (§2a). Both footer
  clauses come from one model or the footer is two voices in one row

> **The red-first test must name which word wins**, or it passes with both
> panels agreeing on the wrong one. **Three voices, not two** — the rail row,
> the participant bar, and the rail *footer count*, which is its own reader of
> the status (`CodingSessionExecutionRail.tsx:307-311`) and can stay raw while
> the other two are fixed. Two cases, the two directions §15(b) names, and
> every case asserts all three:
>
> - signed `completed`, transcript with no terminator, lease fresh → the rail
>   row and the bar both read **`idle`**, and the footer counts it as idle. Not
>   `live` (today's strip), not `Idle` in one place and `live` in the other, and
>   not `All idle` beside a row reading `Working`.
> - signed `running`, lease aged out → the rail row and the bar both read
>   **`No provider answering`**, and the footer does **not** count it into
>   `N working`. Not `Working` (today's rail) and not `1 working` under a strip
>   saying `no provider answering` (walk finding 1's exact frame).
>
> - signed `waiting_for_input`, lease fresh → all three read the waiting word
>   (§2a), and the footer counts it as waiting rather than folding it into
>   `idle`. This is the case the old footer got right by accident, from a second
>   read of the raw status, and the one a naive W1 fold would delete.
> - signed `waiting_for_input`, lease aged out → all three read
>   **`No provider answering`**. Reachability outranks stage; a dead seat is
>   never reported as waiting on a person.
>
> Stated once, as the lane should read it: **an execution whose signed status is
> resting and whose newest transcript item is not a turn terminator must not
> read `live`, `Working`, or `1 working` on any surface.** Both sides agreeing
> is the failure mode, not the pass.
>
> **A second correction to the lead's ruling.** It words this test as *"reads
> `released`, not `live`"*. `released` is wrong: it is the word for
> `kind: "ended"`, which only a `stopped` status produces
> (`codingSessionWireWorkspaceStatus`, `codingSessionWorkspaceModel.ts:192-208`;
> `codingSessionDispositionWord`, `codingSessionUmbrellaModel.ts:589-601`).
> `completed` falls through to `{kind:"idle"}`, so the expected string is
> **`idle`**. Asserting `released` would fail against correct code.

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

> **The spec is ahead of the surface here, and the CLI is ahead of both.** Walk
> finding 5 read the shipped popover in full: `Shared session details` /
> `Channel #engineering` / `Signed projection 3 executions` /
> `Verified source 8b83055307…6ef9fbc0`. No founder, though W6 says founder is
> never unknown and the 44226 genesis carries it. No context, though W12 is on
> the wire and `bee sessions status` prints 27% of 1M for one seat and 12% for
> another from exactly those items (`crew_cmds.rs:884-933`). Nothing here is
> false — it simply answers none of the questions the panel exists to answer,
> while the CLI beside it answers them today. That is the gap D7 closes, and
> the `Verified source` row it already has is the right home for the provider
> key that C1a evicts from the byline.

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
  `no brief on the wire` (the chip renders disabled with that as its title).
  A brief the client holds but may not show follows §10a's elision shape and
  names its size, e.g. `[elided private context: 67 bytes, sha256:a39f1225…]` —
  never a blank panel
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

## 10a. Region J — The way in (channel session list)

*(Added 2026-08-29 — walk finding "what surprised me most", promoted to a
ruling. No mock region: this is the door to every region above, and it can say
the session does not exist when it does.)*

**The pattern to copy, by name.** The best honesty behaviour in the product
today is the elided-context marker, which renders inline in a message as:

```
[elided private context: 67 bytes, sha256:a39f12250cc25119a084511d4d569ae1eb8a60f310da67f442fda1b800fc23cf]
```

It refuses to show a thing **and** discloses how much it withheld and how to
verify it. That is the shape every "we are not showing you something" state in
this spec takes — J1 here, **D5** (tests), **H2** (an unreadable brief). A
refusal that names its own size is trustworthy; a refusal that renders as an
absence is not.

### J1 — Empty channel session list
- cite: no mock element — **new**
- replaces: `ChannelCodingSessionsMenu.tsx:124-128` (`No signed sessions in this
  channel yet.`) and `:236` (*"Sessions started here appear in this list as soon
  as the provider signs its first event."*), plus the trigger's count at `:112`
- reads: `CodingSessionCatalogSnapshot.rejectedAuthorCount` and
  `.invalidSignatureCount` (`codingSessionTypes.ts:151-152`) — **already
  computed** by the trusted ingress (`codingSessionTrustedIngress.ts:600-603`,
  surfaced at `:666-667`, threaded at `useCodingSessionCatalog.ts:92-93`) and
  **read by nothing**. No new plumbing and no wire change
- copy, three states:
  - nothing arrived → existing verbatim: `No signed sessions in this channel
    yet.` / *"Sessions started here appear in this list as soon as the provider
    signs its first event."*
  - events arrived and were all rejected → `No sessions Beekeeper can verify.` /
    *"12 session events in this channel were rejected: 12 with a signature this
    client could not verify, 0 from an author this session does not accept. Run
    `bee events query --kinds 44223 --channel <id>` to see what the relay
    holds."* Counts named separately, each omitted when zero
  - some accepted, some rejected → the list renders as today, with a footer
    line: `3 more session events were rejected and are not shown.`
- walk: seed the channel with correctly-formed 44223 events signed by a key the
  client will not accept; open the trigger. Today it reads `Coding sessions (0)`
  over the "provider has not started" copy — walk finding: three runs were spent
  believing the events were malformed

> **Ruling.** Failing closed is right and stays. The copy is the bug: for an
> operator whose provider signs with a key this client rejects, `No signed
> sessions in this channel yet` says *the provider never started*. It is the
> green-dot-over-a-dead-team lie told by an empty-state string, and the counts
> that make it honest are already sitting in the component's props.

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

**(b) Two models answer W1, and they lie in opposite directions.**
*(Amended 2026-08-29 after Texas's walk, `WALK-2026-08-29.md` findings 1 and 2.
My first diagnosis — "the rail reads raw status, move it onto the strip's
model" — was half the bug and the fix would have spread the other half.)*

Neither of today's two models is the source of truth:

- **The rail under-reports.** `CodingSessionExecutionRail.tsx:398`
  (`executionStatus`) switches on the raw 44223 string; the footer `:307-311`
  recounts `status ∈ {running, starting}`. Neither consults the lease. Walk
  finding 1, on real relay events with aged-out leases: the strip said
  `Poker · no provider answering`, the header said `3 agents · 3 need
  attention`, and the rail said **`Working`** in blue with a footer of
  `1 working` — same execution, same window. Measured proof: the cropped rail
  is byte-identical across a fresh-lease and a stale-lease run
  (`sha256 e0dd7b23…`). Reachability changed; the rail's pixels did not.
- **The strip over-reports.** `codingSessionWorkspaceModel.ts:355-377`: when the
  newest transcript item is not a turn terminator and its `turnId` differs from
  the last terminator's, the function returns `{kind:"working"}` **before it
  reads the signed status at all**. `codingSessionDispositionWord`
  (`codingSessionUmbrellaModel.ts:589-601`) prints that inference as the word
  **`live`** — a reachability word for an activity guess. Walk finding 2: an
  execution whose signed 44223 status is `completed` reads `live` in the strip
  and `Idle` in the rail, and the signed word appears nowhere on screen.

**One correction to the lead's ruling, on the record because it changes a
test.** The ruling says "the strip is not the demoted side." The strip *is*
lease-demoted — `CodingSessionHeader.tsx:593-598` passes
`resolveReachability(…)` into `deriveCodingSessionWorkspaceStatus`, and walk
finding 1 is the proof (the strip moved to `no provider answering` when the
leases aged; the rail did not). The strip's defect is not that it skips the
lease, it is that it **promotes ahead of the signed status** and the lease only
catches that promotion when reachability is *known-unreachable* —
`demoteUnreachable` returns the status untouched on `{known:false}`
(`codingSessionWorkspaceModel.ts:248-252`), which is the honest default and the
common one. So: demoted, but promoted first, and only sometimes caught.

**The ruling this surface builds to (LOCKED):**

1. One function answers **W1** for every panel; the rail and the strip both
   call it.
2. Its source is the **signed 44223 status, demoted by the ephemeral lease** —
   never the transcript.
3. The transcript open-turn heuristic survives only as a **narrowing**: it may
   take a seat the wire already says is live and add *working right now*. It may
   **never promote** a non-live signed status to live.
4. Severity: this is the shape a **killed seat** leaves — a resting last status,
   no terminator, a transcript that looks open. It is the common case.

Owned by **lane 0** (§16), which lands before either UI lane.

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

**Three lanes now, not two** *(amended 2026-08-29)*. RULING 1 requires changing
the one function both UI lanes read, so it cannot sit inside either of them
without making them sequential on a file they share. It becomes a small lane 0
that lands first; lanes 1 and 2 then run in parallel exactly as before. No lane
touches the wire, the relay, the CLI, mobile or web.

### Lane 0 — One answer for W1 (lands first, blocks both)

**Owns, exclusive:**
```
desktop/src/features/coding-sessions/lib/codingSessionWorkspaceModel.ts
desktop/src/features/coding-sessions/lib/codingSessionWorkspaceModel.test.mjs
```

**One move, so no file is shared afterwards.**
`codingSessionDispositionWord` lives in `codingSessionUmbrellaModel.ts` today
(`:589-601`), which lane 1 owns — and the fifth word is a change to that mapper.
**Lane 0 moves the mapper into `codingSessionWorkspaceModel.ts`** and
re-exports it, in the same commit as the word. It belongs there now: the mapper
*is* W1's vocabulary, and W1's source lives in that file. After the move lane 0
owns two whole files and lane 1 owns `codingSessionUmbrellaModel.ts` whole.

**Be clear about what the move costs, because it is not free.** It still has
lane 0 editing lane 1's file once — deleting the mapper and leaving the
re-export. That crossing is safe **because lane 0 lands first and lanes 1 and 2
start after it**: the exclusive-ownership rule exists to stop two lanes writing
one file *concurrently*, and there is no concurrency between a lane and its
predecessor. It is **not** safe because the edit is small. A three-line edit
into a file another lane is writing at the same time is exactly as broken as a
three-hundred-line one, and a lane that reads "it was only a small move" will
make that mistake next time.

**Delivers:** the single W1 function per §15(b)'s locked ruling — signed 44223
demoted by the lease; the transcript open-turn test narrows a live seat and
never promotes a non-live one — **plus W1's fifth word** (§2a): a `waiting` kind
mapped from `waiting_for_input`, demoted by the lease like `working`, and the
two-string mapper that names who is waited on. It changes no component; lanes 1
and 2 only wire it.

**Red first:**
- `a completed execution with an unterminated transcript is idle, not working` —
  the promotion path at `:355-377`, which is the walk's finding 2
- `a running execution with an aged-out lease is No provider answering` — walk
  finding 1
- `a live seat with an open turn still narrows to working` — the heuristic keeps
  its one legitimate job
- `an unknown-reachability live seat is not demoted` — `{known:false}` is not
  evidence of absence (`:248-252`)
- **`a waiting_for_input seat reads the waiting word, not idle`** — today it
  falls through to `{kind:"idle"}` at `:208`
- **`an unreachable waiting_for_input seat reads No provider answering`** —
  precedence, §2a; assert no waiting word appears
- **`the waiting word names the viewer only when the viewer can steer`** —
  `waiting for you` with authority, `waiting for an operator` without
- **`the transcript heuristic never produces the waiting word`** — an open turn
  on a `waiting_for_input` seat does not invent activity

**Done when:** the function's only inputs for the *word* are the signed status
and the lease (plus the steer flag for which of the two waiting strings), a test
proves the transcript cannot raise a seat's state, and `waiting_for_input` is no
longer unmapped anywhere in the file.

Lane 1 and lane 2 below are unchanged except where marked.

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
- **`a turn-block byline names the seat, not the signer`** (C1a) — three seats
  under one provider signer; assert each byline differs, assert no 64-hex or
  truncated-key substring (`/[0-9a-f]{8}…[0-9a-f]{4}/`) anywhere in the block
  header **or its screen-reader line**, and assert the provider key still
  appears exactly once, in D7's `Verified source` row
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
desktop/src/features/coding-sessions/ui/ChannelCodingSessionsMenu.tsx        (J1)
desktop/tests/e2e/singularity-inspector.spec.ts                              (new)
```

**Delivers:** D1–D8, H1–H5, J1. J1 lands here rather than in lane 1 because it
is the same job as D4 and D5 — disclose what we do not know — and lane 1 is
already the larger lane. It touches no file lane 1 owns.

**Red first, one per claimed state:**
- `the tests panel says No test report yet even when a turn claims 3/3 passing`
  — seed an assistant message containing "3 / 3 tests passing"; assert the panel
  never renders a count. **This is the lane's headline test**
- `the plan panel names the seat whose plan it is` — assert `PLAN · Keystone`
- `no plan snapshot renders No plan published` — verbatim, with its description
- **`the rail row, the bar and the footer count agree on the signed word`** —
  four fixtures, three voices asserted in each, and the assertion names the word
  so agreement alone cannot pass it: signed `completed` + unterminated
  transcript + fresh lease → row and bar read `idle`, footer counts it idle;
  signed `running` + aged lease → row and bar read `No provider answering`,
  footer does **not** count it into `N working`; signed `waiting_for_input` +
  fresh lease → all three read the waiting word and the footer counts it
  **waiting, not idle** (§2a); signed `waiting_for_input` + aged lease → all
  three read `No provider answering` and no waiting word appears anywhere. The
  footer is a separate reader
  (`CodingSessionExecutionRail.tsx:307-311`) and a lane can fix the first two
  and leave it raw, so it is asserted explicitly in both cases. See D6's ruling
  block for the one-sentence form
- **`sixteen unnameable edits do not render as no observed changes`** (D3/D4) —
  seed 16 `toolKind:"edit"` items with empty `input`; assert
  `16 edits observed · files not reported`, assert the tab shows no count, and
  assert the string `No observed changes yet` is absent
- **`a channel whose events were all rejected does not say the provider never
  started`** (J1) — seed events with an unacceptable signature; assert the
  rejected-count copy and assert the "as soon as the provider signs its first
  event" sentence is absent
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

---

## 18. 2026-09-01 — consolidation

*(Lane U of the 2026-09-01 team-turn reliability + Mission UI batch, base
`15bbe615`. Verdict being executed: "honest but structurally wrong — every fact
exists; the hierarchy doesn't." Appended, not edited: §§1–17 remain the record
of the 2026-08-29 walk.)*

### 18a. Ownership — one stream home, one rail home, no third copy

| Fact | Stream (causality plane) | Rail (state plane) | Deleted from |
|---|---|---|---|
| Goal | header row 1 subtitle (text only) | Inspector **Current goal** + the edit control (`CodingSessionGoalPill variant="inspector"`) | the goal-pill row above the stream |
| Team roster | row 2 chips — `name · role`, the W1 word, and only the two badges below; **no activity phrase** | Inspector **Team**: model · runtime · generation · seat authority (+ remedy) · last activity | the chip's activity phrase (the live strip is its one home); Context tab's roster duplication (now **Context load**, one number per seat); Header provenance popover seat rows |
| Mission state | typed transaction rows + terminal rows, chronological | Inspector **Mission state**: `<label> · <phase>` and a four-step indicator | the pinned Mission transaction card, and its canonical-chain list |
| Handoff / causality | assignment → report → verdict → acknowledgement rows, each an `A → B` sentence | (none) | the chain list inside that card |
| Live "working" | `CodingSessionLiveActivityBar`, one line per working seat | (none; the chip shows the W1 word only) | the chip's activity phrase **and** the Inspector team row's |
| Delivery status | badge on the **report row** it belongs to | Inspector **Integrity › Delivery**, bounded at 32, newest first | toasts remain transient extras |
| Seat authority | `ungranted` badge on the chip; `unseated` badge on the report row | Inspector **Team** row detail + the exact remedy command | — |
| Observed changes | header row 1 `Observed changes N` | Inspector **Changes** / **Files** | — |
| Tests | (none) | Inspector **Structured tests** (`No test report yet`) | — |
| Context load / usage | (none) | **Context** tab | Header provenance popover context rows |
| Evidence (signed ids) | Trace: `Signed source` under each row | Inspector `SignedSource` disclosures | — |
| Controls | header overflow `⋯` | Inspector: Edit goal, Retry evidence | the duplicate Inspector opener while the rail is open |

### 18b. Transaction-row copy (frozen)

`<Actor> → <Counterparty> · Assignment` · `… · Report` · `… · Refutation` ·
`… · Verdict: <decision>` · `… · Acknowledgement`; terminals
`Mission completed · <Actor>` and `Mission blocked · <Actor>`. The arrow and the
monograms are `aria-hidden`; the row's accessible name is the sentence
`<Actor> to <Counterparty>: <type>`. An unresolvable author reads
`unknown actor`, never a key; the founder reads `You` when no profile resolves.

Weight follows one rule, in
`lib/codingSessionMissionRowGrammar.ts` — `attention` for a blocked terminal, a
refutation, any row carrying a required action, and any row whose delivery kind
is `failed`; `standard` otherwise; `quiet` for lifecycle notices and the
truncation row. Colour never carries a state on its own: every attention row
pairs its hue with an icon and a word.

### 18c. Delivery and seat vocabulary

Frozen in
[`desktop/src/features/coding-sessions/lib/codingSessionMissionContracts.ts`](../../../desktop/src/features/coding-sessions/lib/codingSessionMissionContracts.ts):
`codingSessionTeamWakeDeliveryCopy` (eight kinds, `provider-queued` →
`unknown`), `codingSessionSeatAuthorityCopy` (`granted`,
`created-ungranted`, `unknown`), `codingSessionSeatRepairRemedy`, and the §2a
residual sentence. The UI renders `detail` verbatim and never re-derives it.
`unknown` is a first-class value and is never rendered as any other kind;
`undefined` (no projection supplied) is not `[]` (none observed) — a rail with
no delivery projection reads `Wake delivery unknown`, never "none observed".

**The unseated badge is not seat-authority copy.** §1d's `unseatedReports`
covers a report author with no active seat for the assignment's role — which
includes a seat never created, and a seat held for a *different* role. Neither
has a create receipt behind it, so the badge says
`Report author holds no seat for <role>` (word: `unseated`) and leaves
`Seat created, not granted` to the seat-authority badge, which does.

### 18d. Deleted

- **The pinned Mission transaction card.** Its state line moved to the
  Inspector, its chain list became stream rows, and the component renders
  nothing (the finalizer deletes the file and its mount together).
- **The goal-pill row** above the stream — the header already carries the goal
  and the Inspector now carries the control.
- **The canonical chain list** — three copies of one handoff became one.
- **The provenance popover's duplicated context and seat rows** — the rail owns
  both.
- **The participant bar's own border and background** — it is row 2 of one
  header container, not a sixth chrome band, and the density control lost its
  standalone box for the same reason.
- **The chip's activity phrase** — it made the chip a three-line card, pushed
  row 2 well past the wireframe's 56 px and clipped mid-word on narrow. The
  live strip already says it, once, in full.
- **The inline tool-row wall in Live** — a turn's signed tool items collapse
  into one C2 row (`▸ N execution events · Terminal a · Read b · Edit c`).
  Expanding reveals exactly `N` rows, the existing ones: the count *is* the
  reversibility contract. Brief hides them, Trace lists them. Any class the
  classifier cannot place keeps the classifier's own label (`Tool`,
  `Raw event`) — the bundle never invents a verb, and never prints a zero.

**Two type weights, on purpose.** The stream is read like chat, so a
transaction row's signed summary is `text-sm` and its monogram pair is 24 px
(`missionRowChatBodyClass`); the rail is scanned, so it stays on the `text-xs`
body step. And on an attention row the delivery sentence — including the §2a
residual — is a visible line, not a `title`: the one sentence that explains a
wake nobody can find must not require a hover.

**Rail order:** Current goal · Mission state · **Team** · Changes · Files ·
Structured tests · Accepted plan · Seat-reported plans · Reports · Integrity.
Team is third because the ungranted seat's repair command is the rail's one
action item, and it must be in the first screen rather than below four panels
of file and test detail. `Seat granted` prints as a muted line so the ungranted
one is comparable rather than merely different-by-absence.

**Shape from the grammar, colour from identity and focus.** The card grammar
carries a neutral border and background, so a Mission row that also has an
identity accent or a focus tint must merge the grammar *before* them —
`cn(base, missionRowClassName, accent.border, isHighlighted && …)`. Merged last
it repaints every seat's turn block the same grey and flattens the focus tint,
which is how the accent rail was lost once already.

### 18e. Not adopted from the mock

`+ Member` (overridden in §1 and still overridden), a `Tests 3/3` progress bar
(**W14**: there is no wire source for a test total, so the Inspector says
`No test report yet` rather than drawing a full green bar), and the mock's
`Plan` checklist as an accepted plan (**W15** — the Inspector distinguishes the
seat's own plan from an accepted one, and says so).

---

## 19. 2026-09-01 (late) — what the first live team run showed

*(Lane A3 of batch 2, base `6a683c9e3`. Every ruling below answers a finding
from the TeamRolesV1 run of 2026-09-01 — channel `4aa32763…`, session
`7a374285…` — recorded in `review-2026-09-01/LIVE-RUN-TeamRolesV1.md` and
`review-2026-09-01/batch2/00-BATCH2.md`. Appended, not edited.)*

### 19a. An open turn block sorts at its newest item

Finding 7. `lib/codingSessionMissionStreamModel.ts` `entrySeconds` sorted every
turn block at its start, so an assignment signed *while* a seat was working
rendered below the block it interrupted — the stream told the night's story out
of order. Rule: a block with no terminator sorts at its newest item's
timestamp; a settled block and every transaction row keep their start.
Openness is read from `isCompletedCodingSessionTurnBlock`, never from a clock
comparison. Conversation cannot reach this path — it passes no transactions, so
the projection returns before the merge.

**The limit of the rule.** Two blocks of one stream *can* cross-order: an
earlier block that never received a terminator sorts at its newest item, so if
that item is later than the next block's start it renders below its own
successor. This contradicts the "blocks of one stream can never cross-order"
invariant `lib/codingSessionUmbrellaTimeline.ts:5-16` states, and §19a's
promise is narrower than that invariant: a *settled* block keeps its start, and
an *open* block sorts at its newest item. In practice the provider's crash
recovery synthesises a terminal `Turn result`, so a non-last block without a
terminator is rare — but it is reachable, and the honest reading of "an open
block sorts at its newest item" is that the stale block moves. Recorded rather
than special-cased: a rule that quietly re-pinned such a block to its start
would put a block that may still be writing above rows that came after it.

### 19b. The turn-block byline carries the W1 word, and the rail breathes

Finding 8. Only the roster chips and the live strip said `live`; the stream —
where a reader actually is — said nothing, so a block that was still being
written looked like a record. The byline now renders the seat's W1 word, and
the block's identity rail wears `coding-session-agent-breathe`, both only on a
block that is open and only in Mission.

The word is **passed down** from the surface
(`CodingSessionUmbrellaWorkspace.tsx` builds one map from
`deriveCodingSessionStreamPresence`), never re-derived from the transcript: one
resolution of W1 for the chip, the strip and the byline, so they cannot
disagree. A settled block shows no word — stamping today's disposition on
yesterday's turn would make the record lie the moment the seat moved on.
`generation N` stays as the secondary. Reduced motion is handled by the class
itself (`shared/styles/globals/coding-session.css`).

### 19c. One header, honest bundle verbs, no unsupported "Rehydrated"

Finding 9, three parts.

**The aggregate is gone.** Mission passes `statusLabelOverride={null}`, so the
header badge states the demoted lifecycle word alone (A3/B2). It read
`2 AGENTS · 2 WORKING` one row above the roster chips that say the same thing
per seat — and `IDLE` over a session with a seat mid-turn.

**Bundle verbs name the call.** `lib/codingSessionMissionExecutionBundle.ts`
consults the classifier first, then a seat tool-name table, then ACP's own
`toolKind`, then the classifier's label. A turn
of `Bash`×4 `Read`×2 `Edit`×3 `Grep`×2 reads `Terminal 4 · Read 2 · Edit 3 ·
Search 2`; on the live run the same turn read `Relay 1 · Tool 49`, because the
classifier's harness rules are written for `buzz-dev-mcp` names and a Claude
Code seat calls none of them. A call nothing on the wire describes still reads
`Tool`, the same word its expanded row uses.

`Relay` therefore survives only where the classifier already recognised the
call — a dev-MCP `shell` invocation of `bee`. A seat driven by Claude Code runs
`bee` through `Bash`, which the classifier's buzz-CLI parser never sees
(`agentSessionToolClassifier.ts` gates it on `shell`/`*_shell`/dev-MCP names,
and `BUZZ_CLI_GROUPS` has no `sessions` entry), so that call reads `Terminal`
here while the Audit tab counts it under "Downloads the room". Two names for
one call; not introduced by this change — it read `Tool` before — and neither
file is A3's. Recorded so the next reader does not have to rediscover it
(REVIEW-A3 F6).

**"Rehydrated" is not repeated without a history to point at.** A fresh hire's
first block opened with `Rehydrated — verified session history is available to
this agent` over an umbrella that held no earlier generation of it. The wire is
untouched — the provider's claim is about its own native session — and Mission
omits the row when this umbrella has no prior generation for that execution.
`Started fresh`, `Resumed`, `Loaded` and `Restarted without prior context` are
never touched, and neither is any slug a later provider adds.

### 19d. The record and the room, on one line

Finding 6. `mission.blocked` was used four times as a note — there is no note
or correction verb — and the rail read Blocked, in red, while two seats worked
on for another twenty minutes. Both facts were true; showing only the first
made the surface lie about the session.

The Mission state section now leads with `Blocked (signed) · 2 seats live`:
the fold's state, its provenance, and the roster's own W1 count of working
seats, with the phase indicator under it. `(signed)` appears only where a
signed record establishes the state — `unknown` claims nothing. The count is
never `0 seats live`: no working seat reads `no seat is working`, and no
projection reads `seat liveness not projected`. Nothing here parses the blocker
prose. **A note verb is still missing** and is the real fix; this is the honest
rendering until one exists.

### 19e. Audit — the third rail tab

Finding 11, and Brian's ask ("we need to capture this so it is observable").
`Audit` sits beside Inspector and Context and derives everything from items the
client already holds: `result` items for duration, cost and per-turn `usage`;
`tool` items for names, arguments and result sizes. No new wire kind, no new
endpoint. Row shape is frozen with `bee sessions audit` (`00-BATCH2.md`).

Sections, in order: **Per turn** (seat · started · duration · tools · out ·
cache reads · cache writes · context window, groupable by seat), **Totals**
(per seat, then `Σ this session` with the H5 partial disclosure
`(2 of 3 seats reported)`), **Handed twice**, **Downloads the room**, **Retry
loops**. Bounded at 200 turns and 50 rows per list, each cap announcing itself
in words. Every absent number is an em dash whose hover and screen-reader text
say `not reported`; cost prints only where a driver priced the turn. Where the
driver published no tool count, the count of signed tool items this client
holds is shown with a `*` and a sentence saying so — a different fact, marked
as one.

**Known gap, disclosed on the surface.** Per-turn `usage` is on the wire
(`coding_session_payload.rs`; typed in
`codingSessionTranscriptItemContract.ts`) but the desktop transcript projection
drops it in `buildResultLifecycleItem`, which keeps only `durationMs` and
`costUsd` — the same gap already recorded against context load in
`codingSessionContextLoad.ts`. Until the projection carries it, the token
columns read `—` and the section says why. The model reads `usage` off the item
defensively, so it lights up with no further change the day the projection
publishes it. `contextWindow` is real today: it falls back to the turn's own
`context_window_updated` item.

### 19f. Frozen with `bee sessions audit` (A3 ↔ A1)

The Audit tab and the CLI table derive the same rows from the same items, so
the ledger, a seat and the screen say one thing. Three derivations were
frozen against `crates/beekeeper-cli/src/commands/sessions/audit.rs`:

1. **`toolCalls`** — the driver's own `usage.toolCalls` when the turn's
   `result` carried one, otherwise the count of `tool_call` items that turn
   published. Both measure the same turn; neither is a guess, so this field is
   never absent. The tab marks the fallback with `*` and a hover, because
   *which* measurement a reader is looking at is itself a fact.
2. **`retryLoops.count`** — the **longest** run of consecutive identical
   `(command, result)` pairs, not the total across runs, with a minimum of 3.
   A command a seat came back to twice, hours apart, is work.
3. **`handedTwice.bytes`** — the summed **published** result bytes, with
   `bytesClipped` set when any of them carried the provider's elision marker.
   The provider clips a tool result at 8 KiB, so a clipped total is a floor and
   the tab renders it as `≥`.

Also shared: a room download is named `sessions <verb>` over the four verbs
`status`, `inbox`, `send`, `operation` (never the invocation, never the
object); `costUsd` is reported only when `usage` names a `pricingIdentity`;
and items outside any turn are not a turn — inventing a row for them would put
something in the table that nothing spent.

Not shared, deliberately: the bounds. The CLI folds up to 4,096 items per
execution; the tab bounds what a rail can hold — 200 turns, 50 rows per list —
and announces each cap in words.


### 19g. Fix round 1 (REVIEW-A3)

- **The merge's sort precondition (F1, blocking).** `entrySeconds` is
  deliberately non-monotonic over the narrative, and the two-pointer merge in
  `projectCodingSessionMissionTimeline` requires both of its lists to be sorted
  by the key it compares. One open block early in the stream was hoisting every
  later transaction above every entry after it, and a settled block moved when
  a *different* seat's block settled. The narrative list is now sorted on
  `entrySeconds` just before the merge; `Array#sort` is stable, so ties keep
  the narrative's clamped order.
- **Partial reporting is per turn, not per seat (F2).** A seat with one
  reporting turn in eight counted as "reported" and its Σ printed an eighth of
  the work silently. Totals carry `reportedTurns`, every totals row discloses
  `(1 of 3 turns reported usage)`, and the `*` marker now reaches the Σ.
- **The animation follows W1, not the presence of a word (F3).** `isWorking`
  is the raw wire status; the word is that status *after* reachability
  demotion. A seat reading `no provider answering` was breathing in the stream
  while its own roster chip sat still. The block now takes `{ word, live }`,
  where `live` is the chip's own `status.kind === "working"`.
- **Cost has no price-list gate (F4).** `usage.pricingIdentity` does not exist
  on the wire, so the gate made the column dead and printed `not reported` over
  a cost the driver had reported. `costUsd` is the item's own value.
- **The audit folds only while its tab is open (F5).** The surface hook
  assembles the seat inputs; `CodingSessionMissionAudit` — mounted only when
  its tab is selected — does the fold in its own memo. A single-generation seat
  hands over its transcript by reference. Measured on 2 seats × 2,000 tool
  items with 8 KiB results: the fold is ~18 ms and is now paid only with the
  tab open; an umbrella update with the tab closed costs 0.010 ms.
- **The rail breathes, not the card (F7).** `coding-session-agent-breathe`
  animates a box-shadow, so on the `<article>` it ringed the whole block. A
  live block now renders a dedicated 2 px rail element over the shell's own
  left border and puts the class there.
- **`missionItems` keeps its identity (F8).**
  `resolveCodingSessionMissionBlockItems` returns the very same array unless
  the rehydration row is actually present, and the component memoises it.

**Frozen row shape, amendment 00:20** (`00-BATCH2.md`), matched name for name
with `bee sessions audit`: `toolCallsTruncated` beside `toolCalls` and repeated
on totals; `handedTwice.resultsSeen` with `bytes` null when none came back;
`identicalResults: boolean | null`, null unless the run published a result to
compare; `costUsd` from the item's own value. On Desktop a "cut short"
transcript is an input the caller states (`transcriptTruncated`), and no caller
states it today — the tab folds exactly the projection it was handed, so the
flag is honestly `false` rather than guessed.

---

## 20. 2026-09-02 — the Route rail (batch 2, lane A4)

Brian's ask, verbatim: *"We are running huge margins on the side of our chat.
Could the left margin be filled with some kind of highway system with bridges
and road signs and roads — GPS for a user navigating a session?"*
DESIGN-SPEC §9 answered it; this is what landed and what it cost.

### 20a. What the rail is, and what it is forbidden to be

A **second projection** of rows the Mission stream already renders, laid on a
clock — never a third source. One sign exists per signed 44244 transaction, per
44224 delivery whose frozen copy carries a badge, per created-but-ungranted
seat, per **dated** accepted create. Prose is not an input:
`deriveCodingSessionRoute` takes no transcript, no message and no turn block,
so a turn, a note or an assistant sentence cannot become a sign even by
accident (SURFACES C4). The word on a sign is the word the row uses —
`codingSessionRouteModel.test.mjs` reads each row's own `accessibleLabel` and
asserts the rail printed the same word, type for type, so the two vocabularies
cannot drift apart while the rail cannot import the row builder's private
table.

Files: `lib/codingSessionRouteModel.ts` (the projection and the compression
walk), `lib/codingSessionRouteTypes.ts` (shapes, constants, the duration
sentence, the two width gates), `ui/CodingSessionRouteRail.tsx`,
`ui/CodingSessionRouteScrubber.tsx`, `ui/useCodingSessionRoute.ts`.

### 20b. Unknown is drawn as unknown

- **A hire with no signed time draws no junction.** `CodingSessionExecution`
  carries `operatorPubkey` — who signed the 44221 — but not the create's own
  `created_at`, and `CodingSessionWorkspace.tsx` (which holds the catalog's
  `creates`) is not this lane's file. So the workspace supplies every hire with
  `at: null`, and the model emits no sign and no bridge for it. The road still
  exists; it starts at the seat's first signed transcript moment and reports
  `startedAtSource: "first-signed"` rather than claiming a hire time it does
  not have. **Residual**: thread `creates` (or a `createdAt` on the execution)
  through to the workspace and R2's junctions light up with no model change.
- **A sign whose author no road claims goes off-road**, `road: null`, and still
  appears in the sign column and the screen-reader list. The founder's lane is
  a real key (`CODING_SESSION_ROUTE_FOUNDER_ROAD`), never `null`, precisely so
  an unattributable signed act cannot be drawn as the reader's own.
- **A queued wake with no local observation draws no stretch.** The measured
  span comes from the delivery's `observedAtMs`, which is Desktop's own
  observation, not a receipt time; absent, there is nothing to measure and
  nothing is drawn.
- Roads past seven, and signs past 200 per road, are **counted in words**
  (`+N earlier`, `N more participants not drawn`), never dropped in silence.

### 20c. Where the implementation departs from §9's board

1. **The `You are here` sentence sits above the Now rule, not inside the
   band.** The board's sample band spans twelve minutes and has room for its
   own words; a band over rows signed seconds apart is a few pixels tall, and a
   label pinned to it landed on top of the very signs it was there to locate.
   The tint stays in the map; the sentence — `You are here · 7m to Now` — reads
   in the head block where nothing can collide with it.
2. **The live road head reuses `coding-session-agent-breathe`** rather than a
   new `route-pulse` keyframe. Same 2.4 s tempo, and it is already inside
   `coding-session.css`'s `prefers-reduced-motion` block — a file this lane
   does not own. One animation vocabulary, one reduced-motion guard.
3. **A decision held on a person is a mark on its own sign, not a second
   sign.** R4 lists `Scale` amber as its own element, but every row that names
   a `requiredAction` already has a sign, and a blocked terminal would then
   carry two. The sign keeps its type glyph and wears the amber `Scale` beside
   the word, so one fact renders once and colour is never the only carrier.
4. **The `queued` stretch is one stretch even when moments fall inside it.**
   TeamRolesV1's report wake was queued 4 m 20 s and a hire landed four minutes
   in; the walk counts the span's segments first so the wait is labelled once,
   with its whole measured length, rather than reported twice.
5. **Two files, not one.** `codingSessionRouteModel.ts` alone came to 995
   lines and `CodingSessionUmbrellaWorkspace.tsx` to 1,103; the ceiling is
   split, never raised, so the shapes moved to `codingSessionRouteTypes.ts`
   (re-exported, one import site) and the derivation to
   `ui/useCodingSessionRoute.ts`.

### 20d. Settled turn blocks open at one line (Mission Live)

A settled block — settled by `isCompletedCodingSessionTurnBlock`, the same
terminator test the footer and the stream's ordering use — opens at a byline,
the **first sentence of its first assistant message** (verbatim, cut at 140
characters with an ellipsis, never a summary this client wrote) and an event
count. The count is the reversibility contract, the same one the C2 execution
bundle already keeps: expanding reveals exactly those signed items and nothing
else, and `CodingSessionUmbrellaTurnBlock.collapse.test.mjs` renders the same
block shut and open and accounts for every promised item by kind.

Three blocks never collapse, and each for a stated reason:

- the **working** block, because it is the thing being watched;
- a block carrying an **attention item** — an error, a permission prompt, a
  failed tool call — because a summary line is exactly what would swallow it
  (C5);
- anything in **Brief**, **Trace** or **Conversation**, which are untouched.

Expansion is per block, in memory, and deliberately **not** persisted: a reader
who opened one turn has not asked for every future session to open it.

### 20e. Gates on the fold

The rail is shown only when the workspace **body** is ≥ 1280 px *and* the
gutter left of the reading column can spare 224 px
(`sectionWidth + (railShown ? 224 : 0) − 816 ≥ 224`). The rail's own width is
added back when it is already shown, so the decision does not oscillate on the
pixel where it flips. Below either gate the map folds to a 40 px scrubber that
keeps the attention signs, the band and Now, and names them in its
`aria-label` — folding hides detail, never a fact. Note for anyone writing an
e2e against this: the gate measures the workspace body, and the app's chrome
takes ~310 px, so a 1400 px window leaves the body 1089 and the rail folds.

### 20f. Fix round 1 (REVIEW-A4)

- **A road start is a signed create, or it says it is not (F1/F9).**
  `CodingSessionExecution` now carries `createdAt`/`createEventId` from the
  accepted 44221 the umbrella builder had already resolved to order attachments
  (`codingSessionTypes.ts`, `codingSessionUmbrellaModel.ts`), so the hire
  junction draws from a signed moment. Where no create was observed the road
  still starts at the seat's first provider-authored transcript time — but it
  now **wears an open start cap**, and every road control's tooltip and the new
  `Route roads` screen-reader list say `road starts since its first signed
  sign, not a create`. The previous build drew that case with a filled cap
  identical to a dated junction and disclosed the difference only in a
  document.
- **The compression rule applies to a queued wake too (F2).** A wake is still
  its own measured stretch and its label is still the exact span — the label is
  the fact — but a span longer than five minutes is drawn at the 48 px silence
  height instead of scaled. The §2a residual is a wake that runs to Now until
  the lead or a Desktop returns; scaled, an overnight loss made a ~5,700 px
  rail. `1h 30m` now renders in 48 px and says `1h 30m`.
- **A block that settles under the reader stays open (F3/F4).**
  `settledExpanded` is seeded from `isWorking` and latched, so a turn finishing
  never shuts the block someone is watching, and the expanded block carries an
  `aria-expanded="true"` control that shuts it again. The disclosure now works
  in both directions instead of one.
- **The collapsed count is the rows Live actually reveals (F5).** In Mission
  Live the execution bundle is on by the same density gate that turns collapse
  on, so a turn's tool items arrive as **one** bundle row. The line counts
  narrative rows plus that row (`3 rows`, not `5 events`), and the bundle's own
  count carries the tools one level down — C2's contract, composed with rather
  than repeated.
- **One tab stop, and `aria-pressed` that can be pressed (F6).** Every
  focusable thing in the rail — lanes, signs, road heads, `+N earlier` — shares
  one roving tabindex, so the rail is one stop in the page's tab order rather
  than seven; the sign the rail revealed reports `aria-pressed="true"`.
- **A local clock is drawn as a local clock (F7).** A delivery sign sits on
  `observedAtMs`, which is Desktop's own observation, so it is drawn **hollow**,
  prefixed `~`, and reads `— local time, not signed` to a screen reader. A
  delivery whose observation never landed has no moment to place: it is counted
  on its road as `N undated` instead of vanishing.
- **A hidden sign carries no bridge (F8).** Bridges are filtered by their
  owning sign after the per-road bound, so the map cannot draw 214 arrows for
  200 signs while its own marker says 14 are not shown.
- **Three §9 elements that were missing are built (F10).** R4's dotted tick
  from lane to sign and the filled anchor an attention sign puts on its lane;
  R1's end caps (live filled and breathing, idle a hollow ring, released a flat
  cap); and §9.5's `+N earlier` as a **control** that lifts that road's bound in
  place. Expanding lifts the bound to five times the fold — lifted, never
  removed, so the count keeps telling the truth in both states.
- **Overlapping duration labels stack (F15).** Two wakes on one lane put their
  labels a few pixels apart at `text-2xs`; labels are now laid out top-down
  with a minimum gap. The measurement is never rewritten to make room.
- **The e2e fixture has real durations (F11/F12).** `governedMissionEvents`
  takes an anchor and a spacing; the Route test signs the same mission over
  half an hour, so `route-wide.png` shows roads with length, two hire
  junctions, a compressed `18m` silence and the ticks — a map rather than a
  legend. The spec asserts on kinds that ARE members of the closed set (one
  assignment, one report, two hires, zero refutations, and every `data-kind`
  inside the set), plus the dashed stretch, its duration label, the road-start
  clause, the single tab stop and `aria-pressed`.

- **Two housekeeping fixes, and one residual that is not this lane's (F13/F14).**
  The dead `CODING_SESSION_ROUTE_FOUNDER_ROAD_KEY` export (whose doc sentence
  described a reuse that never happened) and an orphan doc block describing a
  table in another file are deleted. Separately, the required Playwright pair
  ran green three times consecutively, but the `Mission fold hook missing`
  flake the review saw once is a real race and is **still open**: the helper
  evaluates the fold hook straight after `page.reload()`, while the hook is
  installed by a *dynamic* import inside `bootstrap()` — so it lands after the
  `load` event, in a chunk that has to be fetched. Warm cache wins, cold cache
  loses. The remedy is a `page.waitForFunction` guard in
  `tests/e2e/helpers/codingSessionMissionLensAssertions.ts`, which belongs to
  Finalizer B, not to A4.

**Still true, and worth repeating**: the rail needs roughly a **1590 px
window**, not 1440 — the gate measures the workspace body and the app's chrome
takes ~310 px, so §9's own `Route` artboard size does not render the rail.
## 21. 2026-09-02 — the launch dialog becomes one form (lane B3)

The dialog had two tabs, *One session* and *Team*, and finding 12 of the
2026-09-01 live run is what two tabs cost. The Team tab carried **no provider
control at all**, so a team's lead ran on whatever the One-session tab happened
to be showing; that tab's `effectiveModel` was handed to
`resolveCodingSessionCrewSeats` as `fallbackModel` and became the published
model of every seat that pinned none. Role names were typed as free text. The
lead's name was truncated to the point where two identities called Keystone
read identically.

The 2026-09-01 ruling: *"the dialog collapses to one form: goal · who leads ·
governed switch · bench · budget/posture · working directory · access;
readiness shows only blockers, honest unknowns behind a disclosure; nobody is
seated at launch except the lead, or you."*

### 21.1 The form

One column, in the order of the thinking:

| Field | What it decides | Source of truth |
|---|---|---|
| **Goal** | the mission, published as its own kind:44227 and carried as the lead's first turn | one state, whoever leads |
| **Who leads** | you, or one seated identity | managed agents whose pack declares a role |
| **Governed** | genesis + authority chain, or not | **derived from the lead**, never a free switch |
| **Provider and model** | what the lead runs on | the identity's own model, or an override with a reason |
| **Bench** | who the lead may hire, and on which runtimes | published as `bench.identities` / `bench.providers` |
| **Posture, budget, limits** | the kind:44245 record | native `buzz-core` validation, no TypeScript rules |
| **Working directory / worktree** | where the lead runs | unchanged |
| **Access** | the channel or the project | unchanged |
| **Readiness** | blockers inline, unknowns behind *Details* | one pure function, shared with the button |

### 21.2 Two defects removed by construction, not by a check

- **The fallback-model leak.** Closed on the path the form actually takes.
  `resolveCodingSessionCrewSeats` — which took the leaking parameter — had no
  production caller left once the Team tab went, and is **deleted**; removing
  its third argument closed the leak on a dead function while the live path
  still published the model *picker's* default for an identity that declared
  none (REVIEW-B3 F1). `resolveCodingSessionLeadModel`
  (`lib/codingSessionLaunchForm.ts`) now settles it, and it has exactly two
  sources: the identity's own model, or an explicit pick — which is an
  **override** and owes a reason. Neither, and the launch is **blocked**, in
  words, rather than borrowing a model from a control that was never about
  this seat. An empty string is not a model either: a runtime whose models
  command has not answered publishes `defaultModel: ""`, and that used to
  reach the create builder on the governed branch and throw *after* the
  genesis, the goal and the policy were signed (F2).
- **Free-text roles.** The lead's role is read from its pack and rendered. The
  select offers only identities that carry one; the rest are not leads.

### 21.3 Governed is decided by who leads

An agent lead is always governed — a seat holds authority only through a
genesis and an accepted chain, so an ungoverned agent lead would be an agent
with no standing to report, to hire, or to be granted anything. Leading it
yourself is always ungoverned: a governed session exists so somebody who is
*not* you can hold a seat in it, and founding a roster of one, yourself, would
publish three records answering no question. The switch is shown and derived
rather than shown and free, and it states the reason either way.

**Consequence, recorded rather than buried:** the launch form no longer offers
an ungoverned *seated* create. That shape existed (One session + an agent
seat) and is gone. Seating an identity into an existing session is still the
join dialog's job (`AddCodingSessionProviderDialog`), which is where the seat
field with its role box now lives alone.

**Amended 2026-09-08 — the "who leads" question moved out of the dialog.**
"Leading it yourself is always ungoverned" was already half false when it was
written: the one-session path publishes a genesis before every create
(`useNewCodingSessionCreate.ts`, `prepareNewCodingSessionCreate`). It is now
explicitly false for team sessions, which are **founded first** — goal, name,
genesis — before anyone knows who leads (Andy, 2026-09-08). The dialog asks one
team question, an "Agent team" checkbox; unchecked it is today's standard
session, checked it publishes the founding facts and nothing else, and the
lead, the runtime, the bench and the policy are composed on the founded
session's Team card, whose Start seats the lead (or you) under the existing
genesis. A founded session that nobody has started is a first-class state —
"Not started" — on the desktop shelf, in the channel menu and on the phone,
never "Idle" and never "Status unknown". The governed switch of §21.3 is
therefore gone from the dialog; on the Team card it still derives from the lead.

**Amended 2026-09-10 — the dialog is gone; the page asks Solo or Team.**
The "Agent team" checkbox lasted two days. Andy, after using it: "both normal
and team sessions should be configured this way, since we're duplicating most
of the dialog contents in the page anyway … add a 'Solo' / 'Team' selector at
the very top." So **every** "New coding session" click now founds the topic —
one 44226 and nothing else — and lands on the founded page, which is the whole
form: a Solo | Team switch first, then the name (a 44229 when the field is
left), the initial prompt (a 44227 when left), and in Team the lead (agents
only — Team *means* an agent leads; Solo is the you-lead case), the bench and
the policy; then the runtime, where it runs, readiness and Start. "Founding a
roster of one" is therefore no longer a hypothetical this section argued
against: it is what every click does, and the roster's first member is chosen
afterwards, on the page, with the genesis already on the wire. Consequences
this section owes the reader: an abandoned click is a founded session — an
"Untitled session · Not started" row on every device until the page's
**Discard** (a 44230 `closed`) files it under Settled; there is no auto-delete.
The page opens in whichever mode this computer used last (first ever: Solo).
The `ungoverned` unknown's old sentence, "no genesis and no authority chain",
was false on a founded session and now reads "no authority chain, so this
session has no roster, no grants and no signed reports".

### 21.4 Blockers and unknowns are different facts

A **blocker** is inline, always visible, phrased as something to do, and it is
**the** expression the button is disabled on: `canLaunch` is
`readiness.canLaunch`, nothing more. Busy states — preparing a project's
channel, re-checking readiness, a create already in flight — are blockers with
their own sentences rather than a second `&&` beside the button, because that
is what left a disabled control with nothing under it during preparation, which
is the item-79 shape this form claims to have removed (REVIEW-B3 F7).

An **unknown** sits behind *Details*: it never blocks, and it is never dropped,
because "this computer could not check" and "it is fine" are different facts
and only the first is ever an excuse. Today's unknowns: unread project
readiness, a role pack nobody asked about, a role pack this computer does not
hold, an ungoverned session, and a published policy nothing enforces. *A lead
that names no model is not among them* — for an agent lead it is a blocker
(§21.2); leading it yourself it stays an unknown, because the picker is your
own choice.

### 21.5 The policy is stated, not enforced — and says so

`docs/design/portable-team-loop/POLICY.md` §4 in one constant,
`CODING_SESSION_POLICY_STATED_NOT_ENFORCED`, rendered under every policy
control: *"Stated, not enforced. This is published as the mission's intention;
nothing in this build refuses a turn, a token or a push because of it."*
The sentence quotes POLICY.md §4's own words — *"a published policy is a
stated intention, not an enforced limit"* — rather than paraphrasing it, so the
disclosure and the contract cannot soften independently (REVIEW-B3 F9).
`CODING_SESSION_POLICY_ENFORCED_FIELDS` is empty and is the single place that
claim is made; B2.4 gives `budget.turns` its first consumer, and adding that
one string is what flips the row.

The draft is validated **before the genesis is signed**. Every 44245 rule is
core's, which is right, and the consequence was that a cross-field refusal —
`tokensPerSeat` above `tokensPerSession`, say — was first evaluated after the
session was founded and its goal published (F4). A dry run through the same
native builder costs one call and moves the refusal back to where it costs
nothing; and when a step *does* fail after the genesis, the failure says so in
words and names the session and genesis ids, because a founded, seatless
umbrella nobody can find is the same defect as a badge pointing at a message
that is not there.

No policy rule lives in TypeScript. The draft crosses a native boundary
(`desktop/src-tauri/src/commands/coding_session_policy.rs`) and comes back as
the exact unsigned event **plus Rust's own serialization of the content**, so
what the keyring signs is what the decoder read. The TypeScript side is an
exact-field decoder pinned in both directions to a fixture the adapter
generates.

### 21.6 What a launch publishes, said before it is pressed

The plan list names each event and carries its kind integer in `data-kind`,
read from `shared/constants/kinds.ts` rather than typed out, so the sentence
and the wire cannot drift: 44226 genesis, 44227
goal, 44245 policy (only when one was set), **one** 44221 create, 44228 grants,
44220 first turn. A launch that sets no policy publishes none — the withdrawal
record is a deliberate act of taking a policy back, not the default shape of a
session nobody wrote a policy for.

### 21.7 Attribution closes at both ends

The seated create a hire is answered with now carries `hireRef` = the 44221
hire's own event id, and a hire carries `requestedBy`. The founder's host
compares `requestedBy` with the hire's **own signer** before naming anybody:
the relay does not (POLICY.md §5), so a name printed without asking would be a
forgeable claim rendered as fact. Three answers, three sentences — `attributed`
earns the plain name, `unclaimed` names the signer and claims nothing,
`disputed` shows both keys and the words *unverified attribution*. A hired
seat's first turn reads `<requester> · via your Desktop`, **rendered**, and the
branch that produces it consults **three signed facts and no words**:

1. a hire record vouches for this execution — joined by the actor it seated
   **and** the umbrella it was seated into, never the actor alone;
2. the prompt carries no 44220 command id, because the brief travels inside the
   create while any later turn to that seat is a command;
3. the stamped `operatorPubkey` **is** the key that signed that create.

(3) is what keeps the branch from outranking the signer: it does not override
`operatorPubkey`, it requires a particular value of it. The first attempt
derived the same fact from the `[From the lead] ` text prefix and took that
branch before it looked at the signer at all — which rendered a lane message
signed by the **builder seat** as `Your Desktop (hire host)`, and would have
taken a prompt stamped with the reader's own key away from them on the strength
of a string (REVIEW-B3 N1). A content prefix never outranks a signature, and the
Conversation byte-identity fixture now carries exactly that message so the check
can see it.

Batch 1 item 10's `Your Desktop (hire host)` survives for the case it was
written for — a vouch that names no requester. With no vouch at all there is no
hire-host branch to take, and the byline is simply the signer's.

The store the vouch is read from is community-scoped, so it is reset by
`resetCommunityState()`: it became a *rendered* fact this batch, and a hire
answered in one community would otherwise put a lead's name on a transcript row
in the next.

### 21.8 What the deleted tab left behind

`NewCodingSessionCrewTab.tsx` is gone, not merely emptied. Deleting the
component alone left a 385-line module named for a tab that no longer exists,
carrying twelve exports of which ten had lost their last production caller —
including `CodingSessionCrewRoster`, the roster that told the "four rows, one
live agent" lie D14 removed — with a test suite keeping every one of them
green, so nothing would ever have flagged them (REVIEW-B3 F5). The two
survivors are in `NewCodingSessionLaunchNotes.tsx`, named for what they are;
`resolveCodingSessionCrewSeats` is deleted with them, and so are the tests that
were the only thing still calling either.

## 22. 2026-09-02 (batch 3, lane L2) — the wake a person can read, the ruling they owe, the policy, and Prepare

Four rulings, from the second live team run (`review-2026-09-01/LIVE-RUN-TeamRolesV1.md`,
"Live run 2", channel `d3e440ea-…`). Each names the wire it renders and the copy
it uses; the copy tables themselves are frozen in `review-2026-09-01/00-BATCH.md`
§1f and §1g.

### 22.1 An identifier-only wake is read, in one line, wherever a turn is shown

**Wire source.** A 44220 whose whole `action.text` is one of the two objects
`codingSessionTeamWakeText` mints — `{"operationId","type"}` or the five-field
`buzz-team-wake/v1` object. Live: command `cli-wake-v1:4847ff06…`, event
`b0bc2d8f`, text `{"operationId":"4847ff06…","type":"decision.answer"}`.

**Ruling.** That turn renders as the single §1f sentence for its pointer `type`,
never as its own JSON. It is one module (`codingSessionWakeReading.ts`) called
from the one component both lenses render (`CodingSessionTranscript`), so
Mission and Conversation cannot word it differently. The subject comes from the
fold this surface holds; an operation the fold does not hold produces §1f's
unresolved line and **never** a guessed subject. Prose is never re-read: a turn
whose text is not one of the two exact shapes takes the untouched path, so the
Conversation lens moves for wake bubbles and for nothing else.

`{Who}` is resolved from the **author key**, not from the byline. The byline
correctly calls an automatic `team-wake-` command `Beekeeper · team wake` — a
caption for a turn nobody typed — while §1f's sentence wants the person or seat
whose key signed the record.

The raw pointer stays available in Trace and the Inspector, which render the
signed record itself. What ends is a person being handed a JSON object in a
chat bubble.

**Known gap, stated rather than papered over.** Conversation subscribes to no
fold, so its index is empty and its line is §1f's unresolved one. The defect
(raw JSON in a bubble) is fixed in both lenses; the *resolved* subject appears
only where a fold exists. Giving Conversation the resolved line means running
the Mission evidence subscription in a lens that was deliberately built without
one — a ruling, not a lane's call.

### 22.2 The rail says who is waiting, and lists the rulings

**Wire source.** The Rust fold's own `decisions[]` and `waitingOnDecision`
(`invokeCodingSessionTeamFold.ts:82`, `:96`), on the wire since item 105 and
rendered by nothing until now.

**Ruling.** `waitingOnDecision` non-null puts §1g's `Waiting on the founder` /
`Waiting on {Who}` on the Mission state plane. The **fact** is the fold's; the
**qualifier** is this surface's, because only this surface knows liveness: with
the lead holding an open turn the waiting line sits beside the state, and with
no open lead turn it *is* the state line. A mission whose seats are working
while a ruling is outstanding is both things, and saying only one of them is a
rail lying by omission.

The queue is a Mission section of its own, one row per `decisions[]` entry: the
signed question, `Open · held on …` / `Answered by …`, and what it holds up.
`blocks: []` is a real answer from the fold and reads `holds up no assignment
yet` — it is exactly the shape live run 2's founder-held request `2099cdb3` had,
and a row that rendered nothing there would read as "we don't know". Every state
is carried by a word; the amber card is a second carrier, never the only one.
Bounded at 50 rows with the omission disclosed. An absent `decisions` is
`Decisions unknown`, never an empty list.

### 22.3 The Inspector's Context tab renders the policy

**Wire source.** Kind 44245 records for the umbrella, folded by
`beekeeper_core::coding_session_policy::fold_coding_session_policies` through a new
Tauri command (`fold_coding_session_policies_command`), with the accepted
NIP-CSAT chain — receipt stamps included — supplying the same standing rule
(`signer_may_steer_at`) the session provider and `bee sessions policy get` use.
TypeScript decides nothing: not which record won, not who had standing, not
when.

**Ruling.** Three outcomes are three sentences (§1g): `No policy set for this
session`; `Policy withdrawn by {Who}` — a withdrawal is a decision somebody
made, and reading it as "none" erases both the decision and the person; or the
record, printing the fields it sets and, verbatim, the enforcement sentence.

`budget.turns` is the only field anything enforces (POLICY.md §4), so it is the
only row that may say `enforced`; every other row says `stated`. **No field gets
a bar, a meter or a progress ring** — a bar over a limit nothing counts is the
same defect as a status reading Idle over a disconnected provider.

Refused records are listed with author, code and the fold's own reason, as the
CLI prints them. Silence there would make a stranger's competing ceiling
indistinguishable from no record at all.

### 22.4 Prepare confirms the seats this launch has

**Wire source.** None — a local scan of `personas/roles` and the install it
feeds.

**Ruling.** Prepare asks about the lead and the bench **this launch names**, and
says in one line how many other packs it refreshed (§1g). Live run 2, 10:36: a
two-seat launch put six name fields on screen under "Confirm every refreshed
role name", because the installer refreshes every discovered pack and the screen
therefore asked about all of them — it read as role selection and was pack
maintenance.

The install is unchanged: every discovered pack is still refreshed and still
keeps its stored name, so nothing loses a name by being unasked. What changes is
the question. And a refresh that fails is named — refreshed silently is not
refreshed secretly — alongside any roster role the install dropped.

### 22.5 2026-09-02 (fix round 1) — the goal card, the Files card, and what a wake may claim

Four rulings the first four sections did not record, from `DESIGN-CRITIQUE-RUN3.md`
A1/A2 and `REVIEW-L2.md` F2–F5.

**A wake line is composed only when three things agree.** The pointer parses as
one of the two minted shapes (as before), **the turn's own signer is the
operation's author**, and **the pointer's `type` is the record's own kind**.
Any disagreement falls to the unresolved row *with the reason*, never to a
sentence. The old rule composed `{Who}` from the turn's signer and the subject
from the fold's record and never asked whether they were the same identity — so
a member who typed the founder's pointer into the composer was rendered
performing the founder's signed act (`Mallory answered decision 2099cdb3: C…`).
The exact-field parse stops the accident; only the author join stops the act.
`verdict` matches its two signed subtypes and nothing else. Copy in
`00-BATCH.md` §1f, amended today.

**A lens that holds no fold says so.** `— not in this session's records yet` is
a claim about the *session* made from the absence of a *local* index. The
one-seat lens now says `this lens holds no session records; open Mission to read
it.` — the same shape of correction as A1 itself.

**The goal card names which of three things is true.** A 44227 this surface
refuses on identity renders `A goal is published on this channel but it names a
different {founder | session}…`, naming what disagreed; a genuinely absent
record keeps `No accepted mission goal published.`; the reader's own
`unresolved`/`errored` states are L4's, and until they land `absent` still
covers two facts — stated, not hidden. The gate itself is one case-folded,
newest-wins selection instead of an exact map key plus three equality checks in
three files. Live run 3's own miss was **not reproduced** through the real
readers: the class is removed, the incident is not explained. The editor's
control reads `Change goal` when one exists.

**A redaction marker is not a file name.** The Files card asks
`shared/lib/redactionMarker.ts` — the pattern's one owner, no second regex — and
renders `N file edits · paths private to the seat's host`. The bytes and the
digest are **not** re-surfaced in Mission: a redaction disclosed as a redaction
is the point. `CHANGES` still counts such an edit as *named*; rejecting the
candidate upstream in `deriveCodingSessionObservedChanges` is L4's §L4.2.

**A refused record is never silence.** A policy fold that selected nothing but
refused something reads `No policy in force · N refused` with every refusal
listed — `No policy set for this session` is true only when nothing was refused
either. Same rule as the goal card, one surface over.

**A terminal state is never overwritten.** The waiting fact is appended to a
`completed`, `blocked` or conflicted state (`Mission completed · waiting on the
founder`), sits beside a running state whose lead is working, and *is* the state
only for a running mission with no open lead turn. A mission that ended does not
stop having ended because somebody owes a ruling.
---

## 23. 2026-09-02 — Mission's honesty and its density (batch 3, lane L4)

Brian's frame for the lane was one sentence: *"actually look and say hmm."*
The design seat's run-3 critique (`review-2026-09-01/batch3/DESIGN-CRITIQUE-RUN3.md`)
came back with two ranked lists — **A**, what the screen says that is not true,
and **B**, what it costs a person to use — and ruled that both are findings.
These are the rulings that landed. Every one is **Mission-gated**: Conversation's
DOM is frozen by I8 and the masked `outerHTML` diff against the base is the
proof, not a claim.

### 23a. The goal section says which of four things is true (A1)

`CURRENT GOAL — No accepted mission goal published.` was printed over a session
whose 44227 had been signed at launch. Three different facts reached that one
sentence, and only one of them was what it claimed. The reader now states its
own condition and the Inspector prints the sentence for that condition:

| condition | line |
|---|---|
| resolved, none | `No accepted mission goal published.` (unchanged) |
| unresolved | `Goal not read yet.` |
| errored | `The goal for this session could not be read — {message}` |
| rejected | the goal *selection*'s own disclosure (lane L2) |

Wire source: kind **44227**, read by `useCodingSessionGoals`, which now returns
`{goals, errorMessage, resolved}`. `resolved` means the history fetch settled —
**a refusal settles it**, because "we tried and failed" is an answer and a
caller that treated it as still-loading would spin over a relay that had
already replied. An error carries the reader's own message verbatim; a message
the surface paraphrases is one nobody can act on.

**`Set goal` renders only under a settled reader that bound a record, or found
none.** That is the half of A1 that matters most: the control publishes a
*second* 44227, and the UI's answer to a record it failed to read must never be
to make the record worse.

### 23b. A redaction marker is not a file name (A2, live finding 24)

`FILES` printed `[elided private context: 183 bytes, sha256:…]` in the path slot
and `CHANGES` counted it as a second **named** edit. The marker is the host
saying *"I had this and chose not to publish it"* — which is exactly an edit
with no reported file name. `deriveCodingSessionObservedChanges` now rejects
such a candidate upstream, at `normalizeChangedFilePath`, and it falls into
`unreportedEditCount`, which is what that field exists for. The existing string
carries it (`Plus N edits with no reported file name.`); no new copy, and **no
digest disclosure anywhere in Mission** — a redaction disclosed as a redaction
is the whole point. The predicate is `shared/lib/redactionMarker`'s, read from
there rather than written a second time: two regexes are two definitions of one
privacy contract, and that is how readers drift apart.

### 23c. One liveness word, and a stop count that does not call an idle seat live (A4)

Four vocabularies were on one screen for one fact. Two are now fixed and they
are different subjects: the **umbrella** is `Running` (DESIGN-SPEC A3), the
**seat** is `live` (B1), and neither borrows the other's word. `Stop all`
states what it counts:

- button `Stop all (2 seats)`
- title and accessible name `Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.`
- with no live seat, `… — none live. …`; a single seat reads `Stop all (1 seat)` / `Stop 1 seat — live.`
- the confirm's title drops the borrowed word: `Stop 2 seats?`

The liveness split comes from **the same W1 map the roster chips read**
(`deriveCodingSessionStreamPresence`, §19b), handed into the stop-all model
rather than re-derived from status — re-deriving would rebuild the same fault
one layer down. Nothing on this surface prints the word `live` over a number it
did not get from that map.

### 23d. The destructive control leaves the top bar (A6, DESIGN-SPEC A7)

In Mission the six actions collapse into one `⋯` overflow carrying
`Add provider…`, `Stop all (N seats)`, `Close session`, `Reopen session`,
`Export transcript`, `Pop out`, in that order, with `Stop all` taking the
destructive treatment the composer's own `Stop execution` item already uses.
`People` and the surface toggles stay in row 1: they are navigation, not
action. Conversation keeps its flat run of six buttons and its DOM.

### 23e. Who waits on whom, at every width (A5, observer want 1)

`Assignment open 3m 3s · no report yet` existed in exactly one place — the
Route rail's legend — where it named the holder and never the waiter, carried
no clock, and vanished with the rail. `lib/codingSessionMissionOpenHolds.ts`
derives the holds from the same inputs the map is drawn from and returns, per
hold, `{waiterLabel, holderLabel, holding, sinceMs, sinceAt, sourceEventId}`;
every field is null when unknown and the derivation reads **no prose**. Copy,
from DESIGN-SPEC's Observer table:

- `Keystone waits on Ira · Verifier`
- `Assignment open 3m 3s · since 12:33 PM · no report yet` (a report awaiting a
  verdict reads `· no verdict yet`)
- empty state `No open assignments.`

It renders in the Inspector's `Team` row for the holding seat — the one roster
that is complete and survives every width — and a hold whose counterparty
resolves to no seat renders on its own with the words `holder not resolved`,
never against the wrong seat. The rail head prints the *same* sentence from the
same function, so the two cannot drift.

### 23f. The Mission grid: two adjustable rails, and a stream that fills what they leave (B1, B2, B7; Brian 2026-09-02)

The Inspector was draggable, persisted and collapsible; the Route rail was a
hard `w-56` behind a gate the viewer could not influence; and the stream never
gained the space either gave up, because the column kept `mx-auto` and a cap.

1. **The route rail mirrors the Inspector, control for control.** A
   `role="separator"` handle with the same pointer, keyboard and
   `aria-valuenow` semantics, mirrored because this rail is on the left
   (`ArrowRight` widens). Bounds **176–480, default 224**; width under
   `buzz.desktop.coding-session-route-width`, collapse under
   `buzz.desktop.coding-session-route-collapsed`, both strictly parsed —
   an unreadable stored byte is *not* collapsed, because a rail that hid
   itself over one would look exactly like the fold gate misfiring.
   **Collapsed is the existing 40 px scrubber**; nothing new is drawn.
2. **The automatic gate is a floor, not an opinion.** `codingSessionRouteFits`
   asks one question — would the stream drop below **420 px** — and the
   1,280 px body gate and the 816 px reading reserve are gone. The viewer's
   stored choice is never overwritten by a fold; it is restored when the width
   returns.
3. **The stream fills.** In Mission the column drops `mx-auto` and takes
   `max-w-none`. The reading measure moves *inside* the row as a `ch` cap on
   prose (`narrow` 65ch · `wide` 85ch · `full` none; Mission's default is
   `wide` when the viewer has chosen nothing), so a 1,400 px line never happens
   while transaction rows, the Work Log, code blocks and the Audit table use
   the whole column. **Conversation's container cap does not move.**
4. **The Audit table gets room.** Below 400 px of measured width the `PER TURN`
   table's eight columns degrade to **one card per turn** — every value still
   shown, one axis of scroll — and a reader who wants the table widens the
   Inspector with its own handle until the columns return.

Measured at `text-scale 1.25`, both panels at their defaults, from the DOM:

| window | body | route | inspector | stream | column |
|---|---|---|---|---|---|
| 1100 | 789 | 224 | 0 — **sheet** (body < 960) | 565 | 485 |
| 1280 | 969 | 50 (scrubber) | 360 | 559 | 479 |
| 1600 | 1289 | 224 | 360 | 705 | 625 |
| 1920 | 1609 | 224 | 360 | **1025** | **945** |

At 1920 the reading column is 945 px against the 768 px cap it replaced.

The 1100 row is worth stating plainly, because the lane spec's own table got it
wrong: the Inspector becomes a **sheet** below a 960 px body — that is
`isNarrow`, a body-width gate that predates this lane and is not the 420 px
floor — so at 1100 it contributes nothing to the row and the rail correctly
**stays** at 224. There is no width at which an inline Inspector sits beside a
folded rail at 1100. The fold order in rule 2 is therefore half-shipped: the
rail folds on the floor, the Inspector still folds on `isNarrow`. It fires
first at every width tested, so nothing is wrong on screen; it is written down
here rather than left to be rediscovered.

**The Route control acts at every width.** At 1280 the floor has already
folded the rail, so a control that only flipped the viewer's stored flag
changed nothing on screen and never changed its own `aria-expanded` — a
button labelled `Expand route rail` that expanded nothing. Asking for the rail
where both panels cannot fit now **closes the Inspector to make the room**,
and the control's title says so before it is pressed. Collapsing does not
re-open the Inspector: the viewer closed it.

### 23g. The composer stops owning a third of the Mission window (B4, B6)

In Mission the editor opens at one line (`min-h-11`) and grows on focus and on
content; the auto-grow and its 12-rem ceiling are unchanged. The stream's
bottom reserve is **measured from the dock's own height** (a `ResizeObserver`
applied as an inline `paddingBottom`), not the constant `pb-48` that drifted
out of register with it and let the `Add provider` button land on a turn block
when the unreachable notice grew the dock. Conversation keeps `min-h-24`,
`pb-48` and `pb-[34rem]` exactly — the class string is *appended to*, never
substituted, so `twMerge` gives Mission `min-h-11` while Conversation's class
attribute stays byte-for-byte what it was.

**One axis per scroller** (B6): a fenced block in the Mission stream loses its
`max-h-[400px]` vertical cap and keeps its own horizontal scroll. `CodeBlock`
is shared UI, so the cap is lifted from the coding-session call site rather
than from the component.

### 23h. The rail's own words fit, and its map fills it (B5)

A sign takes the rail's remaining width instead of a hard `max-w-32` inside a
224 px rail, so widening the rail widens the sign — B1 and B5 are one fix. The
lane gutter is `16 + (drawnRoads − 1) × pitch + 10`, not a constant reserving
room for seven roads when two are drawn. The `− 1` matters and the first
version of this ruling did not have it: lanes are centred at
`16 + lane × pitch` for lanes `0 … n−1`, so the rightmost ink is one pitch left
of `n × pitch`, and charging the extra lane left the signs still clipped at the
**default** 224 px width — proving B5 at 480 px and leaving it unfixed at the
width everybody runs. A map shorter than its container now fills from
the top, so the blank sits **below** the last sign; §9.2's bottom anchoring
still governs a map taller than the rail.

### 23i. Three sentences

1. **The unreachable notice names the seat** (A7):
   `No provider is answering for {Name · Role} — {detail}. Add a provider to
   the session to continue the work.` With no resolvable name, and on the
   Conversation lens, today's sentence stands.
2. **`1M` gets a noun** (A8): the deck renders `{traits} context` where the
   trait is a context size, and otherwise drops the slot — `High` beside a
   recipient's name reads as a claim about the person. The identity popover
   still carries the whole string under its labelled `Model traits` row.
3. **`STRUCTURED TESTS` stops claiming the wire is silent** (A3):
   `No test report on this session` /
   `Kind 44246 carries checkpoint, gate, finding and phase records. This
   surface does not read them yet.` True before L1 and after it.

### 23j. Residuals, stated

- **The shared turn clock.** `Working for {n}`
  (`CodingSessionTranscriptParts.tsx`) is Conversation's string too and I8
  freezes that DOM. Not retitled here.
- **Three tab groups, three anatomies** (B8). Lens, Brief/Live/Trace and the
  surface tabs are three different shapes for one idea; their components are
  Conversation's as well, so the consolidation is not this lane's.
- **`ROUTE_SCRUBBER_WIDTH_PX` is 40 px against a `w-10` rem width.** Under the
  app's own Cmd +/- zoom the collapsed rail measures 50 px at a 1.25 text
  scale while the fold arithmetic still charges 40. The error runs one way:
  `streamIfExpanded = section − (railWidth − 40)` under-states the stream the
  scrubber is really leaving, so the gate is **conservative** and the rail
  **re-expands later** than it should — about 10 px of window at a 1.25 scale,
  and 80 px at a 2× zoom, where it would move a row of the table. A px
  constant standing in for a rem box should become a measurement.
- **The Audit table lives in the Inspector, not in the reading column.**
  L4.6.4 says the eight `PER TURN` columns render in the reading column when
  Audit is the selected section; what shipped keeps the Audit inside the
  surface host and degrades it to one card per turn below 400 px of measured
  width. The degrade is honest and the viewer can drag the Inspector's own
  separator until the columns return — but the Inspector's default is 360 px,
  so at the default the eight columns are never reached, which is critique B7's
  complaint unresolved for the default viewer. **Follow-on: move the Audit to
  the reading column**, as the rule states.

### 23k. The named follow-on

> **L5 — the Inspector reads kind 44246.** A `fold_coding_session_observations`
> Tauri command beside the existing team-fold wrapper, a TypeScript decoder,
> and four Inspector sections (Tests, Gates, Findings, Phase timing) over
> `crates/beekeeper-core/src/coding_session_observation_fold.rs`. It answers
> observer wants 2, 3, 4 and 7, and it is the only thing that makes 23i.3's
> sentence obsolete. Kept out of L4 because it is the one item that adds a wire
> consumer, and a lane that ships half of one ships a surface that reads some
> observations and silently drops the rest.

---

## 24. 2026-09-02 (batch 3, lane L5) — the Audit tab becomes the observer's screen

Kind 44246 landed in L1 with no Desktop consumer, by design. This is that
consumer, plus the ruling that changed what the kind carries: **no observability
path may depend on asking an agent to report** (Brian, 2026-09-02). Live-run
finding 26 is the case — a seat reported `cargo test -p beekeeper-cli` green after
running one test *file*, a verifier reproduced red on the same patch, and
nothing on any screen could say which was right.

### 24.1 One Rust fold, one exact-field decoder, one adapter-generated fixture

TypeScript never folds 44246. `fold_coding_session_observations_command`
(`desktop/src-tauri/src/commands/coding_session_observation_fold.rs`) calls
`beekeeper_core::fold_coding_session_observations` and flattens its answer
unchanged; `codingSessionObservationWire.ts` checks shape only, refusing one
extra key, one missing key and one `null`-where-a-value-is-required **by name**.
Its test reads `codingSessionObservationFoldAdapterResponse.fixture.json`, which
the **Rust test generates** — the B1c/B3 pattern — so a field the adapter renames
and the decoder does not is a failing Rust test, not a silently empty card.
A `truncated` count renders as truncation, an `ignored` entry is surfaced with
its reason, and neither is ever a silence.

### 24.2 `source: "observed" | "declared"` — provenance, and it never merges

Added to the 44246 body (seven top-level keys now, not six; NIP-CSOB updated).
It is a **required content key**, so this was a breaking schema change: an event
signed by a build that predates it omits `source` and folds to `ignored` with
that reason. Nothing had published a 44246 anywhere at the time, so the cost was
zero -- but the wire doc says so plainly rather than claiming a compatibility it
does not have (REVIEW-L5 F5).
`observed` means a mechanism that was **not the subject** wrote the record;
`declared` means the subject said it about itself. The fold's dedupe keys carry
it — `(author, source, gate)` and `(author, source, findingId)` — so a claim can
never take the place of a measurement.

The session provider publishes `observed` gate rows from the seat's own tool
calls (`crates/beekeeper-session-provider/src/gate_observer.rs`): it pairs a
recognised gate command with its own result, signs the row with the **provider
instance's** key, and publishes it. The seat is not consulted.
`bee sessions observe gate` stays `declared`, and has deliberately no flag to
say otherwise.

**The matcher reads argv, never the text of the line (REVIEW-L5 F1).** The
command is shell-split; anything it cannot read as plain words plus at most one
`cd ... &&` -- a pipe, a redirect, a `;`, a backtick, a `$(`, an unbalanced
quote -- is refused rather than labelled, because the observer cannot say which
segment of a composed line produced the exit it is about to read. `nice [-n N]`
and `env [VAR=VALUE ...]` are stripped; then the head must match a closed table
of `(program, subcommand)` pairs: `cargo fmt|clippy|test`,
`pnpm test|typecheck|lint`, `just check|test|ci`. `grep -rn 'cargo test' docs/`,
`echo "cargo test"`, `git commit -m 'cargo test green'` and `pnpm add ...` match
nothing. The first version matched the gate words *anywhere in the line*, so
each of those minted a provider-signed `observed` **passed** row -- and because
newest-wins keys on `(author, source, gate)`, that false green displaced a
genuine `failed` row as the one both surfaces showed. A seat could bury the
exact failure the record exists to expose.

**`observed` is honoured only when the signer is verified (REVIEW-L5 F2).** The
word is self-asserted on the wire. The fold takes the session's
provider-instance pubkeys -- Desktop supplies each execution's `signerPubkey` --
and a row claiming `observed` from a signer outside that set is folded as
`declared` and listed under `misclaimedObserved`, with the Audit tab naming it:
*"signed by {who}, which is not a provider instance for this session. Shown as
declared."* A caller that resolved no provider set has verified nothing, and the
fold says `provenanceChecked: false` rather than letting the claim pass as a
measurement; `bee sessions observations` is such a caller today, and prints it.

**Newest-wins is never silent (REVIEW-L5 F1).** A later statement replacing an
earlier one for the same key is counted in `truncated.displacedGates` /
`displacedFindings`, and the Audit tab prints *"1 earlier gate statement was
replaced by a later one from the same author."* A `failed` row replaced by a
`passed` one must never read like a gate that had only ever passed.

**Known limit, disclosed on the surface.** A 44246 names the key that *signed*
it and has no field for the subject it watched, so an observed row cannot be
attributed to a seat. The Audit tab groups by author and labels such a block
*"the record names the watcher, not the seat whose work it watched"* rather than
guessing. The remedy is a `subjectPubkey` on the body for observed rows; it is a
named follow-on, not shipped here.

### 24.3 The Audit tab, per seat and per phase

`CodingSessionObservationSections.tsx` renders four sections — checkpoints, gate
rows, findings, phase timing — inside each author's block, above the five
transcript-derived sections the tab already had. Everything carrying a phase
word is in §1e's **declared** order (`planning`, `red`, `green`, `gates`,
`reporting`), never arrival order and never author time; a phase timing whose
name is not a declared word sorts after those that are and keeps its own word.

Every row prints its provenance word -- checkpoints, gate rows, findings **and
phase timings** (REVIEW-L5 F7): the block-level "watched and signed by this key"
line appears only when every row in a block is observed, so in a mixed block a
row without its own word would show no provenance at all.

Gate rows carry `gate`, the outcome word, the **command verbatim**, and the
summary as a monospaced tail folded at eight lines behind `Show all` (the fold
opens; the 200-line ceiling still says what it dropped). `durationMs` is
labelled *the author's own measurement*; a `null` reads `not reported`, never
`0s`. Findings show `findingId`, title, disposition and the count of `refs`,
with a `decisionRef` resolved through the fold this surface holds and otherwise
marked unresolved. Phase timing is a duration list labelled *reported by {Who}*,
with **no bar** — nothing here sets a scale. Every section states its own
emptiness (`No checkpoint yet` / `No gate row yet` / `No finding recorded` /
`No phase timing`), every collection is bounded at 50 rows per seat per section
with the count in words, and no state word is carried by colour alone.

A dangling `assignmentRef` renders in `unresolved` and **excludes nothing** — an
observation cannot deny anything. A seat with observations and no assignment
still gets a block.

### 24.4 `Structured tests` reads gate rows

The card said `Nothing on the wire reports tests` — true when written, false the
day L1 landed (critique A3). It now renders the session's gate rows through the
same component the Audit tab uses, **one shared source**, and with none reads
`No gate row yet` beside the count of 44244 `report.tests[]` entries it holds.
Those entries are labelled *claimed in a report by {Who}* and are never merged
into the gate rows: a claim inside a report and a signed gate row are different
facts.

### 24.5 Conversation resolves a wake pointer from cache, and never fetches

REPORT-L2 §6c left Conversation's §1f line unresolved because that lens
subscribes to no fold, and REVIEW-L2 advised against giving it one. **Ruling:
resolve from cache, never fetch.** Mission remembers the operation index it
folded, keyed by channel + umbrella + genesis + founder, one entry; Conversation
reads that key or nothing. Warm → the resolved §1f line. Cold → §1f's
`this lens holds no session records; open Mission to read it.` No invoke, no
fetch and no subscription in either case. Both sentences are true about what the
lens holds, which is the test.

### 24.6 The policy command stops trusting a TypeScript authority projection

REVIEW-L2 F15, as corrected by REVIEW-L5 F3.
`fold_coding_session_policies_command` receives the signed kind-44228 events
beside the `policyGrants` a TypeScript projection derived, and **refuses any
claimed grant no verified transition supports** — wrong id, wrong grantee,
wrong verb, or **a transition belonging to another umbrella** — listing each
refusal by transition id and reason.

Stated exactly, because the first version of this paragraph overclaimed: the
boundary **cannot invent a grant**. It can still be handed a chain that *omits*
one, and `fold_coding_session_policies` turns standing **off** on a revoke, so
an omitted revoke would have left its grantee steering. It therefore **fails
closed**: any refusal at all drops the whole projection, leaving only the
founder, who needs no grant. `acceptedAt` remains the caller's unverified word,
because acceptance is a fact about a relay receipt this boundary is not given.

It does **not** re-derive the chain: a second implementation beside
`crates/beekeeper-session-provider/src/authority.rs` would be the same drift with
more code. **The real fix — lifting that file into `buzz-core` so provider,
CLI and Desktop share one chain — is a named follow-on. This is a narrowing,
not a closure.**

### 24.7 The Route rail signs for gate rows

A sign exists only for a row the stream itself renders, and a 44246 gate row is
a signed event two surfaces render, so it qualifies. One sign per gate row on
its author's road, its word the row's **own** outcome, and a `failed` row is an
attention sign that survives the fold to the scrubber's 40 px track. Checkpoints,
findings and phase timings get **no** signs: they would flood the gutter L4
made legible. A row with no signed `created_at` draws nothing — the rail never
invents a moment — and a row signed by nobody on the map is off-road, never the
founder's. Per-road limits and truncation are unchanged.

## 25. 2026-09-02 (batch 3, lane L8) — the founder's two acts leave the terminal

Live run 2 had the founder answering three rulings from a terminal while the
app that *showed* every question could do nothing about it. Live run 3 ended
with a verifier's FAIL on the wire and the branch on `main` anyway. Two rulings
follow, and one correction.

**A screen that shows a ruling can take it.** Each open row of the decision
queue carries an Answer control that publishes a real `decision.answer` down
**the same path the launch uses for the 44245 policy**: a Rust command builds
the exact unsigned event
(`desktop/src-tauri/src/commands/coding_session_team_transaction.rs`), the
desktop keyring signs **Rust's own serialization**, and the relay client
publishes it. TypeScript never serializes a 44244 body, so a producer and a
consumer that disagree about those bytes cannot survive one answer. The buttons
are the request's **own** declared `options`, index for index; a request that
declared none offers free text alone rather than inventing choices its asker
never wrote. The control is **disabled with a sentence, never hidden**, when
the viewer is not the party the ruling is held on:

> This ruling is held on {Who}, so only they can answer it. You can read it here.

And a publish is not an answer. Nothing reads `Answered` until a **fold**
carrying the answer arrives; a rejected publish leaves the row `Open · held on
…` and prints the relay's own words behind `The relay did not accept this
answer: `. A queue that flipped on a resolved promise would be the same lie as
a badge with no event behind it.

**An optional key is offered only where the bytes can carry it.** §1k's
`condition` (lane L7) reaches the wire through a capability the Rust side
**measures** — it hands `buzz-core`'s own decoder a canonical answer carrying
the key and reports whether it was accepted. Where it is, the form shows
`Condition (optional)` with a byte counter and §1l's hint; where it is not, the
field is absent and a sentence says so rather than leaving a reader to guess
whether the feature exists. The day L7 lands, the field appears with no edit in
this row. An answered row reads `Answered by {Who} · {choice} · condition:
{text}`, the condition clamped at 200 characters with the remainder disclosed.

**Land is copy-only, and that is a ruling, not an omission.** The Land control
appears on a mission whose newest canonical approving disposition governs a
report naming a `headSha` — and **only** when the push path's own predicate
would admit `(refs/heads/main, HEAD, that sha)`. One function, three callers:
the relay's pre-receive hook, `bee git check --ref`, and this screen. When the
predicate refuses, the control **stays** and prints §1j's refusal string
verbatim behind `Not ready to land: ` — a missing control would leave the
founder guessing which of "not approved", "not read" and "not governed" they
were looking at, and a paraphrase would drift from the words the relay will
actually print at push time. A repository with no `require-verdict` rule says
so and still shows the verdict it read; a view that holds **no** repository
record says *that*, because unknown is not the same fact as ungoverned.

The app never runs the push, for three reasons each disqualifying on its own,
and they are in the copy a person reads rather than in a comment they never
will: the push is irreversible and mutates a ref other people build on; the app
holds no working tree and cannot know which checkout or worktree is meant, and
the main checkout is hot; and `git-credential-nostr` lives in the founder's git
config and shell, not in the Tauri process, so a push from here would fail on
NIP-98 or — worse — succeed under a different identity. What the confirm step
offers is the disposition and report that admit it, the exact command, and a
Copy button. The command names the **commit**, never the branch: a branch name
is not a commit, and two pushes to one branch are two commits of which one was
ruled on.

**Land says which arm made it ready.** *(2026-09-03, lane L21.)* Since the rule
gained two arms, "ready" covers two very different facts and the confirm step
names which one it is standing on. Under arm **(A)** — the viewer is a founder
— it says *"You are a founder of this repository, so the require-verdict rule
on refs/heads/main admits your push with no verdict at all. Nothing here has
ruled on this commit."* That last sentence is load-bearing: a founder's push is
offered over a mission whose only ruling may be `changes-requested`, and a
screen that printed *"approved"* there would be telling a comfortable lie about
a gate. Under arm **(C)** it names both records — the lead's disposition **and**
the verifier who did not refute it — because one without the other is not what
admitted the commit. The refusals name the nearest missing fact in the same
spirit: an approval nobody checked, a verifier who is the report's own author,
and a verified commit pushed by a key that holds no seat are three different
problems and get three different sentences.

*(2026-09-03, lane L27.)* Arm (C) now stands on a **third** record — arm (B)'s
gate rows, observed green on the same commit — so the sentence names those too:
`…, over cargo fmt, cargo clippy, cargo test observed green on this exact
commit.` Naming only the verifier would say half of what admitted the push, and
would leave a reader unable to explain why the very same clearance stops
admitting the moment a gate goes red. Its refusal is the fourth nearest-missing
sentence and names **both** halves — the verifier who did clear it, and the
gate that did not pass — because either alone sends a person looking for the
wrong thing.

The screen also carries whether the mission's **seat roster** reached the rule
at all (`seatsRead`). Arm (C) is a question about roles, so a surface that sent
no seats has not learned that no verifier cleared the report — it has learned
nothing, and must not print the first over the second.

**The Land control names the repository's founders, in every state.** A
repository has **founders**, not an owner: the kind:30617 announcement's
signer, every pubkey in its NIP-34 `maintainers` tag, and every Owner on the
project roster its `project` back-reference names. The control prints them —
by display name where the surface knows one, 8-hex otherwise — beside a
sentence saying whether the viewer is one of them, and it prints that line
whether it is offering the push, refusing it, or saying the rule does not
govern. Until this line existed the screen could say *"not ready to land"*
without ever saying that the rule answers to a key the reader does not hold,
which are two entirely different problems for the person reading it. Two facts
travel with it because both change what the reader should do: **rules are set
by the announcement's signer alone in v1**, so a co-founder editing them makes
their own repository rather than editing this one; and a project roster this
view could not read is disclosed as unread rather than rendered as an empty
one, since a partial founder set presented as whole is exactly the defect
(finding 33) this line exists to prevent.

**Mission agrees with `bee` about a required verifier.** The fold call site
supplies the real `gates.verifierRequired` instead of TypeScript's `false`
default, and the state panel renders the `completion_not_verified` exclusion
with the same sentence the CLI prints. Unknown ≠ false: a surface holding no
44245 passes `false` — exactly as before — and says `No policy record reached
this view, so the fold read no verifier requirement.` rather than implying none
was set.

## 26. 2026-09-02 (batch 3, lane L9) — Pulse knows the mission without asking anyone to report

Brian's ruling, the day two live runs turned on agents forgetting to report:
*"we can achieve this without asking the agents to do it — which I find to
always be the weak link."* Every fact on this surface is produced by a
mechanism under a key, not by cooperation:

| producer | what it produces | where |
|---|---|---|
| the hire host | a `post-commit` hook in the **seat's own worktree** that pushes `HEAD` to `refs/heads/wip/<role>/<assignment-hex8>` under the seat's key, and a `prepare-commit-msg` hook that adds an `Assignment:` trailer | `buzz-core::seat_git_hooks`, `scripts/wip-post-commit.sh` |
| a person, opt-in | the same `post-commit` hook, pushing to `refs/heads/wip/<their-pubkey8>/<branch>` under **their own** key, off until `just wip-share-on` | `lefthook.yml` `post-commit`, `Justfile` |
| the provider | 44246 `gate` rows with `source: "observed"` (Lane L5's field) | consumed here through one adapter |
| the relay | kind 30618 ref state, signed after a push | `buzz-cli::commands::wip_refs` |

**What the installer writes, and the one line it writes outside the worktree.**
Hooks go into the target worktree's own `.git/hooks`, and the config lines go
into that worktree's **own** config — `--worktree` scope, not `--local`, because
in a linked worktree `--local` *is* the shared repository config and arming the
seat would arm the human's checkout with it. Getting a per-worktree config at
all requires one line in the enclosing repository's shared config:
`extensions.worktreeConfig = true`. So the honest property is "the target
worktree's own gitdir, **plus that one line in the shared config**", not "only
the worktree". It is guarded against `core.bare`/`core.worktree`, it is the
minimum git provides, and it is named here because a reader of the seat-install
path should know the enclosing repo is touched once.

**A person's ref namespace is their own key.** An earlier draft used a literal
`human` segment; two people on the same branch then force-pushed over each
other and kind 30618, which records only the last pusher, re-attributed the
loser's commits. `just wip-share-on` resolves the person's pubkey and refuses
rather than guessing, because a ref shared between people is worse than no ref.
It arms sharing and nothing else: it does **not** configure commit signing, so a
person's wip commits are signed only if their checkout was already set up for
`git-sign-nostr`. The push is theirs either way (NIP-98).

**One model, two consumers.** `crates/beekeeper-core/src/pulse_mission.rs` folds and
`render_pulse_mission_lines` composes **every sentence in Rust**. `bee pulse
missions --format compact` prints exactly those strings and Desktop renders the
same strings into elements with testids, re-wording nothing. A golden test
(`crates/beekeeper-cli/src/commands/pulse_mission_tests.rs`) asserts the CLI adds no
prose of its own; a second test asserts no sentence is composed outside
`buzz-core`.

**Two limits this surface states rather than implies.**

1. **Kind 30618 is parameterized-replaceable.** It says where a ref *stands
   now* and who moved it **last** — never a push history. Two pushes to one ref
   leave one row, at the newer SHA, and no row ever claims two.
2. **A wip ref proves a commit was pushed, not that anybody reviewed it.** A
   `landing` row therefore names the verdict or reads `no verdict on the wire
   for this commit`.

**A repo with no ref state reads `No ref state on the wire for this repo`**, and
a member with no wip ref reads `{Who}'s local commits: not shared` — a statement
about **what the relay holds**, never about that person's git config. Wherever a
wip ref is shown, the prune window is disclosed: `Wip refs are pruned when their
branch merges or after 30 days`. `bee pulse prune-wip` plans that prune,
**deletes nothing outside `refs/heads/wip/`**, and keeps any ref whose state
carries no readable date — unknown is not old.

**Gate truth per seat, never collapsed to one.** An `observed` row beats a
`declared` row for the same `(author, gate)` **and says so** — `(observed, over
a declared row)`. Rows sort `failed`, `not-run`, `passed`, then by name, bounded
to four with a visible count. The wire carries no exit code, so none is
rendered. A seat with no row reads `No gate row on the wire for {Who} — a claim
in prose is not a gate row`; absence is stated once, never as a fake row.

**An excluded completion is not a completion.** It renders the exclusion code
and the mission stays `running`; no 44244 record says that in a field, so the
row says it in a sentence.

**Rulings.** `openRulings` lists every unanswered `decision.request`;
`rulingsWaitingOnViewer` is the subset held on the founder when the viewer *is*
that session's founder, or held on the viewer's own pubkey. With no identity the
list is empty and the surface reads `No identity on this surface, so nothing
here can be held on you` — never `0`.

**The overlap row, and the line nothing crosses.** Two **different** umbrellas
whose newest checkpoints name the same path by **exact equality** — never a
directory-prefix guess — produce one row naming the files, the seats and each
commit's age, shown to both sides with nobody asked to look. It is composed only
from events the reader can already query: if either side is unreadable there is
**no row**. **There is no cross-umbrella wake.** A lead may publish a `note`
citing the other umbrella's event and message that lead's pubkey; no record ever
places work on another umbrella's seat. `pulse_overlap.rs` cannot send anything,
and a test asserts that against the module's own code.

**Bounds, stated.** The newest **8 open** sessions by observation time; a ninth
is disclosed by name (`9 open sessions in scope; the newest 8 by observation
time were read`), never dropped. A 44244 `Err` is **one row's** failure — that
row reads `unreadable` with the reason and every other session is untouched.
Wall time comes from 44246 `phase` rows as the authors' own measurements; token
cost is disclosed as absent (`Token cost is not on this surface: Pulse reads no
usage events`) rather than guessed.

**Owed, and not yet on the wire.** Four things this section describes are
built and are not yet producing anything, named here rather than left for a
reader to discover: (a) the provider's own seated-workdir path
(`crates/beekeeper-session-provider/src/session.rs`) is the second site that creates
a seat worktree and does not yet install these hooks — Lane L5's row; (b) the
44246 `checkpoint` body has no field for a commit SHA, so the hook pushes the
ref and publishes no checkpoint, and no overlap row can be computed until L5
lands `checkpoint.files`; (c) the eight sibling keys ride both `bee pulse
missions` and `bee pulse digest`, and the CLI computes an overlap row only when
more than one umbrella is in scope; (d) nothing here has been exercised against
a live relay.

**Wire compatibility.** `PulseDigest`, `fold_pulse_digest`,
`PULSE_DIGEST_SCHEMA` and `conformance/project-pulse-fold/` are untouched, and
kind 44240's event body does not change at all. Mission rows travel as a
**sibling object** adding exactly eight keys — `missionsSchema`
(`buzz-pulse-mission-rows/v1`), `missionScope`, `missions`, `missionErrors`,
`openRulings`, `rulingsWaitingOnViewer`, `overlaps`, `viewerPubkey` — so an
older digest reader ignores eight unknown keys.

## 27. 2026-09-02 (batch 3, lane L12) — which `bee` answered

The 2026-09-01 run ended twice on a founder nudge, and both times the cause
was the same: the runner's transcript read `OPERATION FETCH FAILED … a
correction must preserve its logical subject` — a defect already fixed in the
checkout — because the seat ran the app's bundled `bee`, three fixes behind,
while the orchestrator's own shell ran `…/beekeeper/target/debug/bee`. One
run, two binaries, one channel, and no surface anywhere said so. A path handed
to a seat in prose was honoured only sometimes, because prose is a request and
`PATH` is a fact.

**The host chooses, and says which.** The provider resolves one binary before a
seat starts — the sidecar beside its own executable, failing that the first
`bee` on the inherited `PATH` — sets `BEE` to its absolute path, and prepends
that binary's directory to the seat's `PATH` **once**. Two entries would
restore the ambiguity; one directory is the whole point. Every other entry
keeps its order behind it, including a stale `…/target/debug` that holds a
`bee` of its own: it stops answering `bee` and goes on answering everything
else. We choose which binary answers; we do not confiscate the machine.

**The stamp is observed, never asked for.** The host runs `$BEE --version`
itself, once, and parses it. Nothing in this feature depends on an agent
reporting anything, because every reporting failure in the live runs was an
agent skipping or mis-stating exactly that kind of step. An unparseable or
non-zero answer records the path and the source with the build `unknown` —
and never fails the seat, because refusing a seat over a version string would
be a new way to lose a run.

**Both surfaces say it in words.** The seat card carries one line, never a
colour: `bee 23728227b (bundled)`, `bee 07c470be0-dirty (found on PATH:
/Users/…/target/debug)`, or `bee build unknown` when it did not parse. Project
Pulse's *what is owed* gains **seats running an older bee** — one row per live
seat whose build is not an ancestor of `main`, with its sha7 and how far
behind — and the honest empty state `no seat's build could be compared`, which
is a different sentence from "every seat is current". Ancestry is computed
against a real checkout by the host; TypeScript renders the verdict and never
compares two shas as strings.

**What this does not reach.** `beeStamp` describes the binary the *publishing
host* chose. A seat launched by some other host still gets that host's `PATH`,
and this key would then be describing a choice it did not make — so it is
published only by the host that made it, for the seats it started.

---

## 28. 2026-09-03 (batch 3, lane L22) — a gate row names the commit it observed, and observed-green lands

Arm **(B)** of the verdict-gated push rule, and the four surfaces that had to
change with it. The wire fact is one pair of keys on a kind-44246 gate row:
`headSha` and `dirty`, resolved by the **provider** in the seat's own workdir at
the moment the gate closed. Nothing asks the agent, and the agent cannot sign
the row.

### 28.1 The gate row says which commit, and whether the tree matched it

Every gate row on the Audit tab and in the Inspector's `Structured tests` card —
one shared component, so the two cannot disagree — now carries the commit in
eight hex beside the provenance word, with the whole id on the title. A row that
names **no** commit says `no commit named` in words. It never borrows the
session's current head, and it never borrows a neighbouring row's: reading
absent as "the commit you are looking at" is exactly the comfortable guess this
project treats as a bug, and it is what would let an older green stand for a
newer push.

`dirty: true` renders as a `dirty` mark beside the commit, because a green gate
run over a worktree the commit does not name is not evidence about that commit —
and the push gate refuses such a row, so a reader must be able to see why.

### 28.2 The Route rail's gate sign carries it, in the title

The sign's **word** is still the row's own outcome — the rail never invents a
second vocabulary — but its title is now
`{gate} · {outcome} · at {8hex}` (or `no commit named`), with `(dirty)` when the
row says so. A green sign that could be about any commit is the ambiguity the
key exists to remove.

### 28.3 Land names arm (B) and does not borrow arm (C)'s words

*(Amended 2026-09-03 by lane L27: arm (C) now also stands on these rows, so it
names them — but it never borrows this section's sentence, which says "no person
has ruled on it" and under arm (C) somebody has.)*

`Gates observed green on {8hex} — ready.` followed by the gates that had to be
green, and the plain statement that **no person has ruled on it**. The screen
says which of three very different facts made it ready — *"you are trusted"*,
*"a machine checked this commit"*, *"a verifier cleared the report"* — because
one word over three facts is the class of comfort this project treats as a
defect.

### 28.4 The launch-form switch finally decides what its labels say

`verifierRequired` now has a landing consequence, so the labels L21 rejected as
claiming an effect the switch did not have are the true ones:
**"Observed gates land this mission's work"** (false) /
**"A verifier must also clear it"** (true) / **"Not set"**. The sentence beneath
states both consequences — the completion check *and* the push — and still says
a founder's push is admitted either way.

### 28.5 What the surfaces still cannot say

The Audit tab shows a gate row's commit; it does **not** say whether that row
would admit a push. That answer needs the repository's protection rules and the
mission's seats, which the Land control resolves and the row does not. A badge
on the row promising a landing would be a claim the row cannot check.
