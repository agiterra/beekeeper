# Sessions: design-phase plan

**Status:** dependency-ordered plan for the authority-centric phase.
Companion to [SESSION_NEXT_PHASE_BRIEF.md](SESSION_NEXT_PHASE_BRIEF.md) (input)
and [SESSION_VISION.md](SESSION_VISION.md) (the 2028 horizon it serves).
**Written:** 2026-08-15
**Worktree:** `integration/glue`

Markers, extending the brief's legend:

| Marker | Meaning |
| --- | --- |
| **[VERIFIED]** | Checked against source this session. File cited. |
| **[DECIDED]** | Fixed by Brian's decisions of 2026-08-15. Not re-litigated here. |
| **[REQUIREMENT]** | Must-have from Brian and Andy. |
| **[RECOMMENDED]** | A proposal with reasons. Argue with it. |
| **[GATE]** | Verification that must pass before dependent work starts. |
| **[OPEN]** | Still needs a human decision. |

---

## 0. Spine verdict: authority-first holds, with one amendment

The dependency analysis confirms authority-first. Every phase deliverable
except one (the continuation/fork machinery track, §2 D4) either *is* the
authority model or reads from it: co-input grants, takeover, the fail-closed
fix, the visibility default, and the "spend the host's quota" power all
reference who-may-do-what. Building any of them against today's model —
which is **nothing enforced anywhere** (§1.1) — is the backwork scenario.

The amendment: the first artifact is not the ladder itself but **the place
authority facts live**. Three tracks need the same missing thing:

- the goal **[REQUIREMENT]** needs a durable, human-signed, mutable,
  session-scoped field — and the existing `title` home is confirmed wrong
  (per-generation, provider-signed, immutable **[VERIFIED]** §1.2);
- the authority ladder needs a place where grants are stated;
- fork **[DECIDED]** needs a lineage pointer, and no lineage field exists
  anywhere **[VERIFIED]**.

No session-scoped human-signed record exists today; the closest thing
("founder") is a client-side projection from event ordering. So the phase
opens with **D1: the session charter**, and the ladder (D2) is its first
consumer. One track — D4, the continuation coordinate + history-seeding
machinery — has no dependency on authority in either direction and runs in
parallel from day one; that is where the phase's second seam lives.

Rejected rival spine, for the record: "fork-first" (Appendix A's cheapest
feature). Fork needs no *permission* machinery, but it does need lineage
(D1) and history-seeding (D4b), so it cannot lead; it lands mid-phase for
free once its two dependencies exist.

---

## 1. What verification established

Four parallel source investigations ran on 2026-08-15 against this worktree.
Everything below is **[VERIFIED]** unless marked otherwise.

### 1.1 Brief §3 promotions — the authority baseline is worse than feared

**No signer/authorization check exists on turn commands, anywhere.**
Relay: kinds 44220/44221 map to plain `Scope::MessagesWrite`
(`crates/buzz-relay/src/handlers/ingest.rs:358-368`) behind strict channel
membership (`ingest.rs:715-736`) — any member may publish them. Provider:
the only pubkey comparison is addressing ("is this command for me",
`crates/buzz-session-provider/src/commands.rs:198`). Spec: NIP-CSL says
founder authority "and rendering are consumer concerns"
(`docs/nips/NIP-CSL.md:172-174`) — the gap is by design, not omission.

**Founder authority is a fail-open React preflight.** Founder = signer of
the earliest receipt-joined 44221 bearing the `sessionRef`
(`desktop/src/features/coding-sessions/lib/codingSessionUmbrellaModel.ts:330-350`),
enforced only by disabling UI on the multi-execution umbrella composer
(`codingSessionUmbrellaComposerModel.ts:30-49`) — and it returns
*permissive* on null founder or cold start. The publish path takes no
founder argument (`codingSessionCommand.ts:182-224`). Two extra gaps the
brief did not know: **single-execution sessions have no founder plumbing at
all** (`CodingSessionWorkspace.tsx:317-352`), and **stop/resume/interrupt
are never founder-gated on any surface**. Any channel member can steer,
stop, or resume anyone's session today.

**NIP-AE memories are encrypted — pairwise, with no shared tier.** NIP-44
under the symmetric agent↔owner conversation key
(`crates/buzz-core/src/engram.rs:136,452,488`), plus a relay read gate:
kind 30174 queries must be `authors=[self]` or `#p=[self]`
(`crates/buzz-relay/src/handlers/req.rs:1131-1152`), and engrams are never
channel-scoped. There is **no session-shared vs owner-private distinction**
— memory is strictly per `(agent, owner)` pair. Consequence for takeover:
**an agent's memory does not and cannot transfer to a new owner** without
new design; this phase treats that as a boundary, not a bug (§2 D3).

**Snapshots exclude the nsec; import mints fresh identity.** Enforced by
construction and tests
(`desktop/src-tauri/src/managed_agents/agent_snapshot.rs:19-34`;
`.../snapshot/import.rs:440-517`). **No agent-key backup, rotation, or
transfer mechanism ships**; draft NIP-PMA reserves that ground (relay-
rejected kind 30179). Persistent-agent identity lifecycle is real future
work, not something this phase can assume.

Still unverified, carried forward as gates: re-clonability of the orphaned
workspace test case (G5); host-observability boundary extension (deferred
with must-have #2 **[DECIDED]** out of scope).

### 1.2 The fork/continuation hypothesis: PARTIALLY SHARED

The structure of the observation is right; its premise was wrong. Both
cross-machine continuation and fork are "a `session.create` carrying a
`sessionRef`" (reused vs freshly minted) through the identical
create/receipt/metadata pipeline — the join dialog already reuses the
founding machinery verbatim (`AddCodingSessionProviderDialog.tsx:40-53`).
But **neither starts from durable history plus a code coordinate, because
that machinery does not exist for any path**:

1. **No lineage/parent field** exists in any payload, kind, or doc.
2. **No history-seeding**: nothing ever replays relay-side 44225 transcript
   history into a new execution. The only context recovery is
   `session.resume` via a machine-bound, latest-only, never-published
   `resume_cursor` (`crates/buzz-session-provider/src/state.rs:79-82`), and
   resume is fenced to the same instance + exact generation
   (`commands.rs:208-241`). Cross-machine continuation today starts empty.
3. **No code coordinate in signed events**: `branch` is always null,
   `cwd` is structurally excluded, `repoRef` is carried but never
   dereferenced (`coding_session_payload.rs:279,303-304`).

The relay's git service *can* serve the workspace: clone via smart HTTP,
NIP-98-authed, gated on membership of the repo's bound channel
(`crates/buzz-relay/src/api/git/transport.rs:445-540`). So workspace
recovery is real at repo granularity; the point-in-time coordinate is the
missing piece. **Consequence: the three gaps are one shared work track
(D4), and building them once serves continuation and fork both.**

### 1.3 Native authority machinery — what the ladder can reuse

- **Channel roles, relay-enforced**: `owner/admin/member/guest/bot` in
  `channel_members` (`crates/buzz-core/src/channel.rs:108-119`), mutated
  via kinds 9000/9001 through `validate_admin_event`
  (`crates/buzz-relay/src/handlers/side_effects.rs:309+`) with elevated-
  grant and last-owner protections re-enforced transactionally in the DB
  layer (`crates/buzz-db/src/channel.rs:379-500`).
- **Community roles, relay-enforced**: `owner/admin/member` in
  `relay_members`, mutated via NIP-43 kinds 9030-9032
  (`crates/buzz-relay/src/handlers/relay_admin.rs`); 9032 change-role is
  owner-only; ownership transfer deliberately blocked.
- **Relay-signed membership artifacts** consumers can trust: 39001 (admins)
  and 39002 (roster) are relay-generated materialized views
  (`side_effects.rs:1140-1200`); 13534 is the community membership list.
- **NIP-OA is not a viable grant carrier as-is.** Signature verification is
  real and used at auth boundaries (`crates/buzz-sdk/src/nip_oa.rs:179-236`),
  but **`<conditions>` clauses are semantically evaluated nowhere** — no
  code compares `kind=` or `created_at<` against any event; expiry windows
  are currently decorative. It functions as an all-or-nothing owner-link
  credential. Building session grants on it means first building clause
  evaluation everywhere; a charter-based grant list is cheaper and clearer.
- **Dormant scope lattice**: ingest maps every kind to a required scope
  (`ingest.rs:390-460`) but pure-Nostr connections get `Scope::all_known()`
  (`crates/buzz-auth/src/lib.rs:134-140`). Useful later for relay-side
  hardening; not a v1 dependency.
- **Existing minted-capability pattern**: HMAC invite codes
  (`crates/buzz-relay/src/api/invites.rs`) — the precedent if operator
  invites ever need bearer semantics. Kind 9009 (NIP-29 invites) and kind
  39003 (group roles) are unimplemented stubs.

---

## 2. The decision ladder

```
D1 charter ──► D2 authority ladder ──► D3 takeover
   │                    │
   │                    └──► (later phase: host observability, per D2's lattice)
   ├──► goal UI slice (must-have #1)
   └──► D5 fork ◄── D4b history seeding
                     D4a commit coordinate ─┐
                     D4c workspace clone  ──┴── D4 runs parallel from day one
```

### D1 — The session charter

**Resolves:** brief [OPEN] "where does a durable goal live" and the carrier
half of [OPEN] "what is the authority model." Unlocks D2, D3, D5, and the
goal pill.

**[RECOMMENDED] shape.** A new **addressable event** (NIP-33 kind in
30000–39999, reserved in `buzz-core/src/kind.rs`), `d` = `sessionRef`,
signed by the session owner. Content: `goal` (mutable prose), `roles`
(pubkey → role list), `lineage` (optional: parent `sessionRef` + cut point
as `cs-target` + `eventSeq`), schema version. Rationale:

- Addressable events give mutability-with-LWW for free on existing relay
  infrastructure; NIP-33 write-conflict semantics already have a CLI exit
  code (5).
- `d = sessionRef` makes the session **relay-legible for the first time**
  (today no tag carries `sessionRef` at all — `NIP-CSL.md:170-174`), which
  is what later relay-side enforcement and server-side session queries hang
  off. This is deliberate: it revisits NIP-CSL's "consumer concern" stance
  for the one event where identity must be authoritative.
- Grants live **inline in the charter, not as separate grant events**, for
  v1: one authoritative document, atomic role changes, trivial revocation
  (publish new charter), no reconciliation across event types. Split later
  only if write contention demands it.

**The hard sub-decision — charter succession.** NIP-33 replacement is
per `(kind, pubkey, d)`: a charter signed by a different pubkey is a
*different* addressable event, so takeover (D3) produces two candidate
charters and consumers need a deterministic pick rule. **[RECOMMENDED]**:
the authoritative charter is the founder's, unless a charter signed by a
provable community admin (per relay-signed 39001/13534, gate G3) exists
with a later timestamp and an explicit supersession pointer to the prior
charter's event id. Adversarial review of this rule is gate G4 — it is the
one place a design error becomes a hijack primitive.

**Bootstrap and legacy:** sessions created before charters exist have none.
Consumers fall back to today's founder projection; D2's enforcement treats
charter-less sessions under a grace rule (see D2). New creates publish a
charter at founding — same signing ceremony, no new UX step.

**Seam:** protocol + kind registry + NIP-CSL amendment on
`feature/coding-sessions` (upstreamable); desktop charter read/write on
`feature/coding-sessions`; any coupling to project containers/access on
`integration/glue`.

**Slices (small bites):** (1) kind + payload type + NIP-CSL amendment;
(2) publish-at-create + charter subscription in the catalog;
(3) goal rendering — the must-have #1 pill — pinned atop the session
surface; (4) goal editing (owner-only via D2, but ship read-only first).
Slice 3 is the phase's first user-visible win and needs nothing from D2.

### D2 — The authority ladder and its enforcement points

**Resolves:** [OPEN] authority model; must-have #3 (invite + co-input);
the co-input quota finding; [OPEN] simultaneous steering (first slice).

**[RECOMMENDED] ladder** — four rungs, deliberately few:

| Role | Powers |
| --- | --- |
| **viewer** | Read everything. Default for every channel member **[DECIDED]** — steering is gated, knowledge is not. Not a grant; membership *is* viewership. |
| **operator** | Prompt and interrupt executions. Presence-gated: live co-input only while the host is connected **[DECIDED]**; fork is the offline path. Carries the distinct power **"may spend the host's quota"** — surfaced in the grant UI and in the transcript, never bundled silently. |
| **owner** | Operator powers + stop/resume/end + charter edits (goal, grants) + inviting operators. Initially the founder. |
| **admin** | Community owner/admin (native `relay_members` role). Exactly one session power: takeover (D3). Not a day-to-day steering rung. |

No-bottleneck check **[DECIDED]** (the governing principle): viewers need
no one's permission; fork needs no one's permission; owner absence is
covered by admin takeover; admin is a *set* (community can hold several);
and if the owner is unavailable to grant operator status, fork remains the
permissionless path. No rung makes one person the only path to progress.
The residual single-point: only the owner grants operator status day-to-day
— accepted, because fork bounds the damage of an absent owner and takeover
bounds permanent absence. **[OPEN]** if Brian wants co-owners instead,
the charter's `roles` map already permits multiple owners; the succession
rule (G4) must then be reviewed for multi-owner conflicts.

**[RECOMMENDED] enforcement points, in trust order:**

1. **Provider-side, fail-closed — the real gate.** The provider already
   subscribes to commands; it additionally fetches/holds the session's
   charter and drops any 44220/44221-resume/stop whose signer is not
   authorized for that power, publishing a receipt with a new error code
   (`UNAUTHORIZED_OPERATOR`) so the record shows the attempt honestly.
   This closes today's hole at the point of execution — the machine that
   would do the work refuses. Charter-less legacy sessions: enforce
   founder-only (the current projected rule), not fail-open.
2. **Client preflight — UX only.** Extend the existing composer gating to
   all surfaces, closing the N=1 gap and the never-gated stop/resume/
   interrupt paths, and flip the null-founder fallback from permissive to
   restrictive once charters flow. Preflight is never the security
   boundary.
3. **Relay-side — deferred hardening.** With `d = sessionRef` charters the
   relay *could* validate 44220 signers, stopping transcript pollution
   (unauthorized commands the provider ignores still land in the channel
   record). Deferred: it makes the relay parse session semantics NIP-CSL
   currently keeps out of it. Revisit after D2 ships; the dormant scope
   lattice (§1.3) is the natural mount point.

**Simultaneous operators, first slice only:** ACP permits one in-flight
prompt per process; "an operator who cannot see a queue cannot reason."
V1: provider publishes queue-position receipts (turn accepted/queued/
started) so concurrent operators see ordering; no merging, no locking.
Richer coordination is explicitly out of this phase.

**Presence gating** reuses native presence (`buzz-pubsub`); gate G6 checks
its reliability as an authorization signal (vs advisory UI signal — if
presence flaps, the provider-side rule should be "operator grants work
while the provider is running," with presence shaping UX only).

**Seam:** provider enforcement in `crates/buzz-session-provider` +
payload/error-code additions in `buzz-core` (`feature/coding-sessions`,
upstreamable); grant UI + preflight unification in desktop
(`feature/coding-sessions`); any dependence on project-access roles on
`integration/glue` against `feature/project-access`.

**Slices:** (1) error code + provider-side founder/owner check (charter
optional — enforce founder from creates as interim, closing the live hole
early); (2) charter-driven roles in the provider; (3) grant/invite UI +
quota-spend labeling; (4) preflight unification incl. N=1 and stop/resume;
(5) queue receipts.

### D3 — Admin takeover

**Resolves:** [OPEN] single administrator / succession; permanent absence
**[DECIDED]**.

Takeover = a supersession charter signed by a community admin (D1's
succession rule), inheriting **identity, history, membership, and the
right to attach new executions** — nothing else. Honest bounds, all
verified this session: the old owner's executions die with their access
(provider state is host-local); uncommitted work is gone and the surface
says so plainly **[DECIDED]** (no magic recovery); **agent memory does not
transfer** (pairwise-encrypted, §1.1) — a takeover surface must say that
too, or users will assume the agent "remembers."

**Transcript visibility [DECIDED]:** an authority change is a fact about
the session. **[RECOMMENDED]**: a dedicated lifecycle receipt kind/payload
("ownership assumed by X under Y's admin authority, supersedes charter Z")
rendered as a first-class timeline row, not a silent charter swap.

**[OPEN] for Brian:** contestability on return. Recommendation: takeover is
final; the returner is re-granted (possibly ownership back) by the current
owner or an admin. Symmetric "contest" machinery is complexity without a
named user.

**Gate G3 before build:** confirm a desktop consumer and the provider can
*verify* community-admin status from relay-signed artifacts (39001 is
channel-admins; 13534 is the community membership list — check role
carriage) without a new query surface.

**Seam:** succession rule + receipts on `feature/coding-sessions`;
admin-role sourcing is the one place this phase touches
`feature/project-access` semantics — coupling lands on `integration/glue`.

### D4 — Continuation machinery (parallel track from day one)

**Resolves:** [OPEN] what a fork carries (the mechanics half); machine-death
continuation **[DECIDED]** target: "continuation from a committed
coordinate," honest about loss.

Three sub-tracks, independently shippable, no dependency on D1/D2:

- **D4a — commit coordinate in signed events.** Provider records, per
  generation (start and on each turn end), the workspace's `HEAD` commit +
  a dirty flag + the resolved `repoRef`, into 44223/44224 payloads.
  Honesty rule: a dirty tree is recorded as dirty, never inferred away.
  Gate G2: confirm the provider can cheaply and safely read git state from
  the cwd it already holds (it runs on the host with workspace access; the
  env fence is about keys, not cwd — expected yes, verify).
- **D4b — history seeding.** A translator in `buzz-session-provider` that
  projects a session's 44225 transcript (queryable from the relay, §1.2)
  into initial context for a fresh execution's first turn. Gate G1 per
  adapter: how much seed context each ACP runtime accepts and in what form
  (initial prompt vs session/load); define the truncation policy against
  the 32 KiB event cap realities. This is the single highest-leverage
  build in the phase: it converts "continuation starts empty" into
  continuation, and it is fork's payload.
- **D4c — workspace provisioning.** When `repoRef` names a bound relay
  repo (30617), a fresh machine's attach flow offers clone-at-coordinate
  (D4a's commit) instead of requiring a pre-existing local project mapping
  (`projects.json`). Gate G5: run the live test on Brian's orphaned
  `agiterra/Hallway` sessions — the named recovery case becomes the
  acceptance test.

**Machine-death surface honesty [DECIDED]:** when a continuation attaches
at a committed coordinate, the surface states what was lost (dirty flag at
last record, generations that ended without receipts) — "an honest record,"
not "as if nothing happened."

**Seam:** all of D4 is upstreamable provider/protocol work on
`feature/coding-sessions`. It is the natural track for a second
person/agent to own end-to-end while D1–D3 proceed.

### D5 — Fork

**Resolves:** [OPEN] fork carriage and lineage; must-have #4 as
re-scoped **[DECIDED]** (work decomposition first, offline escape second).

Fork = mint new `sessionRef` + publish charter with `lineage` pointer
(D1) + new execution seeded from parent history at the cut point (D4b) +
optionally clone-at-coordinate on another machine (D4a/c). Forker signs
their own create on their own machine and provider login — the documented
economic model **[VERIFIED, INTENT honored]** — so fork needs **no
permission from anyone**, which is precisely what makes presence-gated
co-input acceptable **[DECIDED]**. Divergence is permanent: no merge-back,
no reconciliation, no result flow to the parent **[DECIDED]**. Lineage is
one pointer, parent→none, fork→parent; the UI renders "forked from X at
seq N" from the charter and nothing more.

Master-plan decomposition case (the primary intent): fork inherits the
*full* parent history up to the cut, then each fork's charter gets its own
goal ("slice: relay-side validation") — the goal field doing decomposition
work from day one.

**Seam:** `feature/coding-sessions` for create-path + lineage; desktop
fork affordance likewise. **Slices:** (1) fork-on-own-session (copy, same
machine — no D4c needed); (2) fork-across-machines (needs D4c).

### D6 — Sequenced out of this phase, with their tripwires

- **Host observability pills (must-have #2) [DECIDED out]:** falls out of
  D2's lattice later as a viewer-facing, capability-gated projection.
  Tripwire: D2's role table should not hard-code its power list — new
  powers (e.g. "see host topology") must be addable without a schema break.
- **Session-shared agent memory ([OPEN] in brief):** current truth is
  clean — nothing is shared, everything is pairwise-encrypted. A shared
  tier is real protocol design (new kind or key-sharing), not a flag flip.
  Do not promise it in any takeover or collaboration UX this phase.
- **Persistent agents / managed-agent unification ([OPEN] `agentRef`):**
  the remote-agents tier has the invariants a persistent-agent design
  needs, but agent-identity lifecycle (backup/rotation/transfer) does not
  exist and NIP-PMA is a reservation. Tripwire in D1/D2: **grants are
  pubkey→role with no human-only assumption**, so an agent can hold an
  operator grant the day one exists — the 2028 surface where agents are
  participants must not require re-cutting the charter schema.
- **Compute mechanisms relationship ([OPEN]):** untouched. D4c's clone
  provisioning is written against "a machine with a conforming launcher,"
  not against the desktop specifically, to stay compatible with the
  remote-agents direction.

---

## 3. Remaining verification gates

| Gate | Question | Blocks | How |
| --- | --- | --- | --- |
| **G1** | How much seed context does each ACP adapter accept, via what mechanism (initial prompt / `session/load`)? | D4b, D5 | Empirical per-adapter test with the Claude + Codex adapters already in use. |
| **G2** | Can the provider read `HEAD`/dirty state from the session cwd safely and cheaply? | D4a | Read `buzz-session-provider` spawn/fence path; prototype. Expected yes. |
| **G3** | Can provider + desktop verify community-admin status from relay-signed artifacts (13534 role carriage, 39001 scope)? | D3 | Source check + live query. |
| **G4** | Does the charter succession rule resist hijack (backdated charters, fake supersession, multi-owner races)? | D1→D3 | Adversarial design review before the rule ships; a Tamarin sketch is optional but NIP-AB sets the precedent. |
| **G5** | Is the orphaned `agiterra/Hallway` workspace re-clonable from the relay git service? | D4c acceptance | Live test against Brian's state; also validates the recovery narrative end-to-end. |
| **G6** | Is presence reliable enough to gate authorization, or UX-only? | D2 operator rung | Read `buzz-pubsub` presence semantics; decide "presence shapes UX, provider-running shapes authorization" if flappy. |

---

## 4. Seams — dividing the work across the branch model

Per the vision doc's branch roles and working rules (no direct commits to
`integrated`, upstreamable session work on `feature/coding-sessions`,
coupling on `integration/glue`, no checkpoint pushes):

| Track | Branch | Independent? |
| --- | --- | --- |
| D1 charter kind + NIP-CSL amendment + payloads | `feature/coding-sessions` | Starts immediately. |
| D2 provider enforcement + error codes | `feature/coding-sessions` | Slice 1 immediately (founder-interim, before charter); rest after D1. |
| D2 grant UI / preflight unification | `feature/coding-sessions` | After D1 slices land. |
| D3 takeover + admin sourcing | `feature/coding-sessions` + coupling on `integration/glue` | After D2; G3/G4 first. |
| D4a/b/c continuation machinery | `feature/coding-sessions` | **Fully parallel from day one.** Natural second owner. |
| D5 fork | `feature/coding-sessions` | After D1 + D4b. |
| Docs, this plan, cross-feature glue | `integration/glue` | Continuous. |

Two people (or a person and an agent fleet) can run the phase as two
tracks: **authority track** D1→D2→D3 and **machinery track** D4→D5, meeting
at D5. Every slice above is PR-sized; nothing requires a big-bang landing.

---

## 5. 2028 guardrails — what this phase must not preclude

Checked against the vision's product invariants:

1. **Grants are pubkey-based, never human-only** — agents become operators
   without schema surgery (invariant 1, 3; direction §4.2).
2. **Charter is provider-neutral** — nothing in it names a runtime;
   provider selection stays an execution choice (invariant 10).
3. **Authority changes are attributable timeline facts** (invariant 5).
4. **Viewer-by-default with gated steering** implements "shareable under
   project authority" without leaking execution state — host paths, cwd,
   and quota internals stay out of signed content (invariants 7, 9).
5. **The power list is extensible** so observability, approval powers, and
   future agent-specific powers slot into the same ladder (invariant 4).
6. **No rung makes one person the only path to progress** — the governing
   principle, enforced by construction: permissionless fork, admin-set
   takeover, membership-derived viewership.

## 6. Must-have coverage map

| Must-have | Where |
| --- | --- |
| #1 permanent goal at top | D1 slices 3–4 (first user-visible win) |
| #2 pills: sub-agents/processes/ports | Deferred **[DECIDED]** — D6 tripwire keeps the door open |
| #3 default me; invite + co-input while connected | D2 operator rung, presence-gated; quota-spend labeled |
| #4 fork to own machine / copy own session | D5 (slice 1 same-machine, slice 2 cross-machine) |
| #5 shared visibility; invite collaborators | Viewer-by-default **[DECIDED]** + D2 grants |
