# Sessions: design-phase plan

**Status:** dependency-ordered plan for the authority-centric phase. **v3.**
Companion to [SESSION_NEXT_PHASE_BRIEF.md](SESSION_NEXT_PHASE_BRIEF.md) (input)
and [SESSION_VISION.md](SESSION_VISION.md) (the 2028 horizon it serves).
**Written:** 2026-08-15 (v1); revised same day (v2) after external review.
**Worktree:** `integration/glue`

**Revision log.** v2 incorporates a Codex review of v1 (2026-08-15). All seven
findings accepted, two as outright design-error fixes: (1) the authority core
is now an immutable session genesis plus an append-only authority chain with
relay acceptance, replacing v1's single LWW charter, whose author-scoped
NIP-33 replacement could not survive takeover; (2) "spend the host's quota" is
no longer a session-owner-grantable power — it requires a host-issued,
target-scoped lease intersected with session authority. Also per review:
commands gain a session/authority binding; relay enforcement moves from
deferred hardening to a collaboration release gate; fork cuts become
multi-stream manifests; recorded commits are not called recoverable until the
relay confirms the object; history seeding preserves provenance; and D6 gains
a recovery matrix. Where v1 text survives, it is unchanged.

v3 (same day) adds §7: reconciliation of an independent idea-level review
(gpt-5.6-sol, unanchored — given only the vision and brief), reconciliation
of newly discovered shipped work on `upstream/feature/project-access` and
`upstream/feature/builtin-shell`, and a document status index. §§0–6 are
unchanged from v2 except this note. Execution now lives in
[SESSION_EXECUTION_PLAN.md](SESSION_EXECUTION_PLAN.md).

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
fix, the visibility default, and the quota question all reference
who-may-do-what. Building any of them against today's model — where **no
session-role authorization exists beyond channel membership** (§1.1) — is the
backwork scenario.

The amendment: the first artifact is not the ladder itself but **the place
authority facts live**. Three tracks need the same missing thing:

- the goal **[REQUIREMENT]** needs a durable, human-signed, mutable,
  session-scoped field — and the existing `title` home is confirmed wrong
  (per-generation, provider-signed, immutable **[VERIFIED]** §1.2);
- the authority ladder needs a place where grants are stated and where the
  *current* grant state is unambiguous;
- fork **[DECIDED]** needs lineage, and no lineage field exists anywhere
  **[VERIFIED]**.

No session-scoped human-signed record exists today; the closest thing
("founder") is a client-side projection from event ordering. So the phase
opens with **D1: session genesis + authority chain**, and the ladder (D2) is
its first consumer. One track — D4, the continuation coordinate +
history-seeding machinery — has no dependency on authority in either
direction and runs in parallel from day one; that is where the phase's second
seam lives.

Rejected rival spine, for the record: "fork-first" (Appendix A's cheapest
feature). Fork needs no *owner-approval* machinery, but it does need lineage
(D1) and history-seeding (D4b), so it cannot lead; it lands mid-phase once
its two dependencies exist.

### 0.1 Two meanings of "subscription," four separate authorities

The whole phase assumes subscription-backed execution in both senses, and the
senses must never blur:

- **Buzz relay subscriptions** deliver live activity. **Persisted signed
  relay events establish durable session truth.** No machine may need to have
  been online when a grant, turn, takeover, or transcript event was
  published: on reconnect it backfills stored events, then resumes the live
  subscription. Every design below is judged against this rule.
- **AI-provider billing subscriptions** stay personal, exactly as the
  documented economic model requires: an execution runs on the launcher's
  machine and provider login; a teammate continuing or forking runs a *new*
  execution on *their* subscription. No shared keys, no metering.

From these, four authorities that are related but never interchangeable:
(1) relay event access (channel membership), (2) provider billing (the
launcher's own login), (3) **session participation** (may steer this
session — session authority, D1/D2), (4) **host-quota spend** (may drive
this specific execution — host authority, D2's lease). v1 conflated (3) and
(4); v2 does not.

---

## 1. What verification established

Four parallel source investigations ran on 2026-08-15 against this worktree.
Everything below is **[VERIFIED]** unless marked otherwise.

### 1.1 Brief §3 promotions — the authority baseline, precisely

**No session-role authorization exists beyond channel membership.** The relay
does enforce strict active channel membership on all coding-session kinds
with no open-channel fallback (`crates/buzz-relay/src/handlers/ingest.rs:715-736`)
— that is real and stays. Beyond it: kinds 44220/44221 map to plain
`Scope::MessagesWrite` (`ingest.rs:358-368`) — any member may publish them.
Provider: the only pubkey comparison is addressing ("is this command for me",
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
missing piece — with the caveat (v2, review finding) that **a locally
observed commit is not the same fact as a relay-recoverable commit** (D4a).
**Consequence: the three gaps are one shared work track (D4), and building
them once serves continuation and fork both.**

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
- **Relay-signed artifacts consumers can trust** — the precedent for D1's
  acceptance receipts: 39001 (admins) and 39002 (roster) are relay-generated
  materialized views (`side_effects.rs:1140-1200`); 13534 is the community
  membership list; 30618 is relay-signed git ref state.
- **NIP-OA is not a viable grant carrier as-is.** Signature verification is
  real and used at auth boundaries (`crates/buzz-sdk/src/nip_oa.rs:179-236`),
  but **`<conditions>` clauses are semantically evaluated nowhere** — no
  code compares `kind=` or `created_at<` against any event; expiry windows
  are currently decorative. It functions as an all-or-nothing owner-link
  credential. Building session grants on it means first building clause
  evaluation everywhere; an explicit authority chain is cheaper and clearer.
- **Dormant scope lattice**: ingest maps every kind to a required scope
  (`ingest.rs:390-460`) but pure-Nostr connections get `Scope::all_known()`
  (`crates/buzz-auth/src/lib.rs:134-140`). Useful for the relay-side
  enforcement slice; not a D1 dependency.
- **Existing minted-capability pattern**: HMAC invite codes
  (`crates/buzz-relay/src/api/invites.rs`) — the precedent if operator
  invites ever need bearer semantics. Kind 9009 (NIP-29 invites) and kind
  39003 (group roles) are unimplemented stubs.

---

## 2. The decision ladder

```
D1 genesis + authority chain ──► D2 session capability ∩ host lease ──► D3 takeover
   │                                      │
   │                                      └─► relay enforcement slice
   │                                           (collaboration release gate)
   ├──► goal UI slice (must-have #1)
   └──► D5 fork ◄── D4b history seeding
                     D4a commit coordinate ─┐
                     D4c workspace clone  ──┴── D4 runs parallel from day one
```

### D1 — Session genesis and the authority chain

**Resolves:** brief [OPEN] "where does a durable goal live" and the carrier
half of [OPEN] "what is the authority model." Unlocks D2, D3, D5, and the
goal pill.

**[RECOMMENDED] shape — three artifacts, not one.** v1's single LWW charter
is dead: NIP-33 replacement is author-scoped, so a takeover would strand
authority on the departed owner's coordinate, competing admin takeovers
would race on signer-controlled timestamps, and packing the security grant
list into one LWW document with the goal invites lost updates. Instead:

1. **Session genesis** — a new **immutable regular event** (kind reserved in
   `buzz-core/src/kind.rs`), operator-signed, carrying `sessionRef`, the
   channel `h` tag, and optional initial goal text. **Founder = genesis
   signer**, cryptographically, replacing the "earliest receipt-joined
   create" heuristic — founder authority no longer depends on trusting
   relay history ordering. The relay validates the `sessionRef`↔channel
   binding at ingest and rejects a second genesis for the same `sessionRef`
   in the same channel; `d = sessionRef` alone is not channel scoping, the
   `h` tag plus that validation is.
2. **Authority chain** — a new event kind for **append-only authority
   transitions** (grant, revoke, ownership transfer, takeover). Each
   transition references the genesis event id, the previous accepted
   transition's event id, and a sequence number. **The relay validates each
   transition against the chain and the signer's standing (owner for
   grants/revocations; community admin for takeover) and publishes a
   relay-signed acceptance receipt; the latest accepted transition is the
   canonical authority head.** Relay acceptance is the serialization point
   that makes competing takeovers impossible to race. This is a deliberate,
   narrow revision of NIP-CSL's "consumer concern" stance — the relay
   becomes authority-aware for exactly one chain per session, with 30618
   and 39001/39002 as the native relay-signed-artifact precedent. (The
   alternative — pure consumer-side chain-selection rules with no relay
   acceptance — was considered and rejected: every consumer and provider
   would re-implement conflict resolution, and takeover races would still
   need *some* serializer.)
3. **Goal/title metadata** — a separate **addressable LWW event**
   (owner-signed, `d = sessionRef`, `h` tag), prose only, **no security
   payload**. Lost-update risk on prose is acceptable; on grants it is not.

**Bootstrap protocol (the charter/create race, made explicit).** Publish
order: genesis first, then the 44221 create, which gains a `genesisRef`
field carrying the genesis event id — the cryptographic link between the
two. The provider resolves `genesisRef` (backfill query by id) before
acting; unresolved after a bounded retry window → lifecycle receipt with a
new error code (`GENESIS_NOT_FOUND`), never silent execution. Partial
publication is therefore safe in both orders: a create without a resolvable
genesis fails closed and visibly; a genesis without a create is an inert
record. Retried creates carry the same `genesisRef` and dedupe as today.
**Legacy sessions** (pre-genesis) get the founder projection as fallback
with founder-only enforcement (D2), not fail-open.

**Seam:** protocol + kind registry + NIP-CSL/NIP-CSC amendments on
`feature/coding-sessions` (upstreamable); relay validation + acceptance
receipts in `buzz-relay` on the same branch; desktop read/write likewise;
coupling to project containers/access on `integration/glue`.

**Slices (small bites):** (1) genesis + goal kinds, payload types, spec
amendments; (2) relay validation of genesis + `sessionRef`↔channel binding;
(3) publish-at-create + `genesisRef` + provider resolution; (4) goal
rendering — the must-have #1 pill (needs only slices 1–3 and no authority
chain); (5) authority-transition kind + relay chain validation + acceptance
receipts. Slice 4 is the phase's first user-visible win.

### D2 — The authority ladder: session capability ∩ host lease

**Resolves:** [OPEN] authority model; must-have #3 (invite + co-input);
the co-input quota finding; [OPEN] simultaneous steering (first slice).

**The two-authority principle (v2, review finding).** v1 let the session
owner grant "spend the host's quota." Wrong: session ownership and
execution-host authority are independent. An execution runs on the
launcher's machine and provider login; if Alice attaches her provider to
Brian's session, Brian must not be able to authorize Charlie to spend
Alice's subscription. Effective permission to drive a *live execution* is
an **intersection**:

- **Session capability** (from the authority chain, D1): this pubkey may
  steer *this session* — send turns, interrupt.
- **Host lease** (issued by the execution launcher's key): this pubkey may
  control *this specific `cs-target`*, optionally with an expiry. Spending
  the host's quota is precisely what a lease authorizes — it is
  host-grantable only, never owner-grantable. Default: launcher only. One
  affordance: "allow this session's operators," which tracks the operator
  set at the current accepted authority head so the launcher grants once,
  not per person. A budget field is a possible later refinement, not v1.

In the common case the owner *is* the launcher, both grants come from the
same key, and the intersection is invisible — one person clicks one thing
and it feels like today. Continuation and fork never need a lease at all:
they are new executions on the continuer's own machine and subscription.

**[RECOMMENDED] ladder** — four rungs, deliberately few:

| Role | Powers |
| --- | --- |
| **viewer** | Read everything. Default for every channel member **[DECIDED]** — steering is gated, knowledge is not. Not a grant; membership *is* viewership. |
| **operator** | Session capability to prompt/interrupt. Driving a live execution additionally requires that execution's host lease. Live co-input is presence-*shaped* (UX) but lease-*authorized* — see below. |
| **owner** | Operator powers + stop/resume/end + goal edits + authority transitions (grant/revoke/transfer). Initially the founder (genesis signer). Multiple owners are possible as chain transitions — and unlike v1's LWW charter, the chain makes that operationally true. |
| **admin** | Community owner/admin (native `relay_members` role). Exactly one session power: takeover (D3). Not a day-to-day steering rung. |

No-bottleneck check **[DECIDED]** (the governing principle): viewers need
no one's permission; fork needs no owner's approval; owner absence is
covered by admin takeover; admin is a *set*; and if the owner is
unavailable to grant operator status, fork remains the escape hatch. The
lease adds no rung-shaped bottleneck: it binds only live co-input into
someone's running execution — the one act that spends another person's
subscription — and continuation/fork route around it entirely. **[OPEN]**
if Brian wants co-owners by default; the chain supports it, and G4's review
must cover multi-owner transition races.

**Presence [resolves G6 by design]:** presence shapes UX (who appears
steerable now); it never carries authorization. Authorization = session
capability at the accepted head ∩ unexpired host lease ∩ provider running.
A flapping presence signal can therefore mislead a UI but never mint or
revoke a permission.

**Enforcement points, in trust order:**

1. **Provider-side, fail-closed — protects the checkout.** The provider
   validates each command's signer against the accepted authority head and
   its own host-lease state, and drops unauthorized commands with a receipt
   (`UNAUTHORIZED_OPERATOR`) so attempts are honest record. Charter-less
   legacy sessions: founder-only. This ships first and closes the live
   hole.
2. **Relay-side — a collaboration release gate, not indefinite hardening
   (v2).** Provider enforcement alone still lets unauthorized 44220s into
   the channel record: transcript pollution and command/receipt
   amplification. Before *invitations ship*, commands must become
   relay-checkable: **44220 (and lifecycle 44221 actions) gain `sessionRef`
   and the accepted authority-head event id** (and a lease reference where
   one applies). Today a command carries only a `cs-target`
   (`docs/nips/NIP-CSC.md`), which the relay cannot map to a session
   without a new index — the binding must ride in the event. The relay
   already validates the chain (D1), so checking a command against the
   head it references is cheap. Sequencing: D2 slice 1 (provider) may ship
   alone; the grant/invite UI may **not** ship before this slice.
3. **Client preflight — UX only.** Extend the existing composer gating to
   all surfaces, closing the N=1 gap and the never-gated
   stop/resume/interrupt paths, and flip the null-founder fallback from
   permissive to restrictive. Preflight is never the security boundary.

**Simultaneous operators, first slice only:** ACP permits one in-flight
prompt per process; "an operator who cannot see a queue cannot reason."
V1: provider publishes queue-position receipts (turn accepted/queued/
started) so concurrent operators see ordering; no merging, no locking.
Richer coordination is explicitly out of this phase.

**Seam:** provider enforcement + error codes in `buzz-session-provider` /
`buzz-core`; command-binding fields + relay check in `buzz-relay` +
NIP-CSC amendment; grant/lease UI + preflight unification in desktop — all
`feature/coding-sessions`; project-access coupling on `integration/glue`.

**Slices:** (1) provider-side founder/owner check with `UNAUTHORIZED_OPERATOR`
(interim, before the chain — closes the live hole); (2) chain-driven
session capabilities in the provider; (3) host-lease issue/verify (launcher
key, target-scoped, expiry); (4) command binding fields + relay
enforcement — **the collaboration release gate**; (5) grant/invite/lease
UI with quota-spend labeling; (6) preflight unification incl. N=1 and
stop/resume; (7) queue receipts.

### D3 — Admin takeover

**Resolves:** [OPEN] single administrator / succession; permanent absence
**[DECIDED]**.

Takeover = an authority transition of type `takeover`, signed by a
community admin, validated and accepted by the relay like any other
transition (D1) — admin standing checked against `relay_members` (gate
G3). Relay acceptance serializes competing takeovers: first accepted wins,
the rest are rejected against the moved head. The new owner then continues
the *same* chain — no stranded coordinate, no author-scoped replacement
problem.

Inheritance **[DECIDED]**, honest bounds all verified this session:
identity, history, membership, and the right to attach new executions —
nothing else. The old owner's executions die with their access (provider
state is host-local); host leases they issued die with their executions;
uncommitted work is gone and the surface says so plainly **[DECIDED]** (no
magic recovery); **agent memory does not transfer** (pairwise-encrypted,
§1.1) — a takeover surface must say that too, or users will assume the
agent "remembers."

**Transcript visibility [DECIDED]:** an authority change is a fact about
the session. The acceptance receipt for a takeover transition is rendered
as a first-class timeline row ("ownership assumed by X under Y's admin
authority"), never a silent head move.

**[OPEN] for Brian:** contestability on return. Recommendation: takeover is
final; the returner is re-granted (possibly ownership back) via a
subsequent transition by the current owner or an admin. Symmetric
"contest" machinery is complexity without a named user.

**Gate G3 before build:** confirm relay-side takeover validation can read
community-admin standing directly (`relay_members`), and that desktop
consumers can *display* provable admin status from relay-signed artifacts
(13534 role carriage / 39001 scope) without a new query surface.

**Seam:** transition type + validation on `feature/coding-sessions`;
admin-role sourcing is the one place this phase touches
`feature/project-access` semantics — coupling lands on `integration/glue`.

### D4 — Continuation machinery (parallel track from day one)

**Resolves:** [OPEN] what a fork carries (the mechanics half); machine-death
continuation **[DECIDED]** target: "continuation from a committed
coordinate," honest about loss.

Three sub-tracks, independently shippable, no dependency on D1/D2:

- **D4a — commit coordinate in signed events, as separate facts (v2).** A
  locally observed `HEAD` proves nothing about recoverability — the commit
  may exist only in the dead machine's object database. The provider
  records, per generation start and turn end, **five separate facts**:
  observed local commit, dirty flag, repository coordinate (`repoRef`),
  whether the commit is confirmed reachable in the relay repository, and
  the verification timestamp. "Recoverable" is claimed **only** on relay
  confirmation; an unpushed commit renders as "recorded, not recoverable —
  push to make this coordinate durable." (This gives Buzz discipline
  **[DECIDED]** a truthful UI lever: the session surface itself shows when
  work is one push away from surviving the machine.) Honesty rule: dirty
  is recorded as dirty, never inferred away. Gates: G2 (provider reads git
  state from the cwd it already holds — expected yes, verify) and G7 (a
  cheap relay object-reachability check — what surface exists or is
  needed).
- **D4b — history seeding as a provenanced continuation package (v2).** A
  translator in `buzz-session-provider` projects a session's 44225
  transcript into seed context for a fresh execution — but **not** by
  flattening prompts, tool output, and agent messages into one anonymous
  prompt, which destroys attribution and invites prompt-injection
  ambiguity. The package is structured: each item keeps its author pubkey,
  kind, event id, and role (operator turn / agent output / tool result),
  and the projection records which events it derived from. Adapters that
  accept structured context get structure (gate G1, per adapter); adapters
  that only accept a prompt get a clearly attributed rendering with
  provenance markers, under an explicit truncation policy sized against
  the 32 KiB event-cap realities. This is the single highest-leverage
  build in the phase: it converts "continuation starts empty" into
  continuation, and it is fork's payload.
- **D4c — workspace provisioning.** When `repoRef` names a bound relay
  repo (30617) and D4a confirms the coordinate reachable, a fresh
  machine's attach flow offers clone-at-coordinate instead of requiring a
  pre-existing local project mapping (`projects.json`). Gate G5: run the
  live test on Brian's orphaned `agiterra/Hallway` sessions — the named
  recovery case becomes the acceptance test.

**Machine-death surface honesty [DECIDED]:** when a continuation attaches
at a committed coordinate, the surface states what was lost (dirty flag at
last record, unreachable final commits, generations that ended without
receipts) — "an honest record," not "as if nothing happened."

**Seam:** all of D4 is upstreamable provider/protocol work on
`feature/coding-sessions`. It is the natural track for a second
person/agent to own end-to-end while D1–D3 proceed.

### D5 — Fork

**Resolves:** [OPEN] fork carriage and lineage; must-have #4 as
re-scoped **[DECIDED]** (work decomposition first, offline escape second).

**The cut is a manifest, not a pointer (v2).** An umbrella session holds
multiple executions, each with its own per-generation sequence, plus
session-lane conversation — a single `cs-target` + `eventSeq` cannot name
"full parent history up to the cut." The fork's genesis (D1) carries a
**cut manifest**: parent community/channel and genesis event id; the
accepted authority-head event id at fork time; per-execution transcript
high-water marks (each `cs-target` → last included `eventSeq`); a cut
point for session-lane messages; the source signing identities whose
events the fork history includes; and an access-inheritance statement.
Lineage remains one-directional and final: no merge-back, no
reconciliation, no result flow to the parent **[DECIDED]**; the UI renders
"forked from X at cut M" from the manifest and nothing more.

**Permission, stated precisely (v2).** Fork requires **no owner approval**
— that property is what makes presence-shaped co-input acceptable
**[DECIDED]**, and the forker signs their own genesis and create on their
own machine and subscription (§0.1). But it is still bounded by read
authority: the forker must be able to read the parent (channel
membership), must have repo access for the code coordinate, and the fork's
destination must not widen access to copied history. **[RECOMMENDED]** v1
rule: a fork lives in the parent's channel, inheriting its membership
boundary exactly; cross-channel forks are deferred until an
access-comparison rule exists.

Master-plan decomposition case (the primary intent **[DECIDED]**): fork
inherits the parent history up to the cut via D4b's provenanced package,
then each fork's goal event gets its own slice statement ("slice:
relay-side validation") — the goal field doing decomposition work from
day one.

**Seam:** `feature/coding-sessions` for manifest + create-path; desktop
fork affordance likewise. **Slices:** (1) fork-on-own-session (copy, same
machine — no D4c needed); (2) fork-across-machines (needs D4c).

### D6 — Sequenced out of this phase, with their tripwires

- **Host observability pills (must-have #2) [DECIDED out]:** falls out of
  D2's model later as a viewer-facing, capability-gated projection.
  Tripwire: neither the chain's grant vocabulary nor the lease schema may
  hard-code a closed power list — new powers (e.g. "see host topology")
  must be addable without a schema break.
- **Session-shared agent memory ([OPEN] in brief):** current truth is
  clean — nothing is shared, everything is pairwise-encrypted. A shared
  tier is real protocol design (new kind or key-sharing plus re-encryption
  rules), not a flag flip. Do not promise it in any takeover or
  collaboration UX this phase.
- **Persistent agents / managed-agent unification ([OPEN] `agentRef`):**
  the remote-agents tier has the invariants a persistent-agent design
  needs, but agent-identity lifecycle does not exist and NIP-PMA is a
  reservation. Grants and leases are **pubkey→power with no human-only
  assumption**, so an agent can hold an operator grant the day one exists.
  Before an agent *runs persistently across hosts*, Buzz needs: identity
  backup/recovery or deliberate rotation; a run lease fencing two restored
  copies of the same identity (the same shape as D2's host lease and the
  managed-agent `at-most-one-live-instance` invariant — design them to
  rhyme); memory ownership/sharing/re-encryption rules; and
  revocation/successor records. None of that is this phase; all of it now
  has a named home.
- **Compute mechanisms relationship ([OPEN]):** untouched. D4c's clone
  provisioning is written against "a machine with a conforming launcher,"
  not against the desktop specifically, to stay compatible with the
  remote-agents direction.

**Recovery matrix (v2)** — what survives what, current vs target. This is
the honest one-page answer to "the session survives the loss of a machine":

| Asset | On machine death (this phase's target) |
| --- | --- |
| Human identity (nsec) | Restored by the human (NIP-49 backup / NIP-AB pairing). Never touched by session machinery. |
| Provider identity (per-machine) | **Intentionally lost** unless keyring migrates with the host image; a new machine mints a new provider. Session identity does not depend on it. |
| Persistent-agent identity | **Intentionally lost today** (no backup/rotation — §1.1); NIP-PMA ground. D6. |
| Session authority (genesis + chain) | **Restored** — relay-persisted, backfillable by any client (§0.1). |
| Transcript / receipts / metadata | **Restored** — relay-persisted. |
| Committed code, relay-confirmed (D4a) | **Restored** — clone-at-coordinate (D4c). |
| Committed code, unpushed | **Lost**, and the record says which commit and that it was never confirmed reachable. |
| Dirty working tree | **Intentionally lost [DECIDED]** — surfaced plainly; teammate abandons, recreates, moves on. |
| Live executions + host leases | **Lost** — new executions attach under the surviving authority chain on new machines/subscriptions. |
| Agent memory (NIP-AE) | Survives *as ciphertext* on the relay for the original (agent, owner) pair; unreadable by successors. D6. |

---

## 3. Remaining verification gates

| Gate | Question | Blocks | How |
| --- | --- | --- | --- |
| **G1** | Per ACP adapter: does it accept structured seed context or prompt-only, how much, via what mechanism (initial prompt / `session/load`)? | D4b, D5 | Empirical test with the Claude + Codex adapters already in use. |
| **G2** | Can the provider read `HEAD`/dirty state from the session cwd safely and cheaply? | D4a | Read `buzz-session-provider` spawn/fence path; prototype. Expected yes. |
| **G3** | Can relay-side takeover validation read community-admin standing (`relay_members`), and can desktop display it from relay-signed artifacts (13534/39001)? | D3 | Source check + live query. |
| **G4** | Do the chain validation rules resist hijack (forged genesis for an existing `sessionRef`, transition replay, competing takeovers, multi-owner races, head-reference stripping on commands)? | D1→D3 | Adversarial design review before the rules ship; a Tamarin sketch is optional but NIP-AB sets the precedent. |
| **G5** | Is the orphaned `agiterra/Hallway` workspace re-clonable from the relay git service? | D4c acceptance | Live test against Brian's state; validates the recovery narrative end-to-end. |
| **G6** | ~~Presence reliability for authorization~~ **Resolved by design (v2):** presence is UX-only; authorization = capability ∩ lease ∩ provider-running. | — | Closed. |
| **G7** | What surface confirms a commit is reachable in the relay repository (existing ref/object query vs new endpoint)? | D4a | Read `buzz-relay` git module (30618 ref state, upload-pack negotiation); prototype the check. |

---

## 4. Seams — dividing the work across the branch model

Per the vision doc's branch roles and working rules (no direct commits to
`integrated`, upstreamable session work on `feature/coding-sessions`,
coupling on `integration/glue`, no checkpoint pushes):

| Track | Branch | Independent? |
| --- | --- | --- |
| D1 genesis/chain/goal kinds + spec amendments + relay validation | `feature/coding-sessions` | Starts immediately. |
| D2 provider enforcement + error codes | `feature/coding-sessions` | Slice 1 immediately (founder-interim, before the chain); rest after D1. |
| D2 command binding + relay enforcement (collaboration release gate) | `feature/coding-sessions` | After D1 slice 5; **blocks the invite UI**. |
| D2 grant/lease UI / preflight unification | `feature/coding-sessions` | After the release gate. |
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

1. **Grants and leases are pubkey-based, never human-only** — agents become
   operators without schema surgery (invariants 1, 3; direction §4.2).
2. **Genesis, chain, and goal are provider-neutral** — nothing in them
   names a runtime; provider selection stays an execution choice
   (invariant 10).
3. **Authority changes are attributable timeline facts** — chain
   transitions and their acceptance receipts render in the record
   (invariant 5).
4. **Viewer-by-default with gated steering** implements "shareable under
   project authority" without leaking execution state — host paths, cwd,
   quota internals, and lease terms stay out of what viewers need
   (invariants 7, 9).
5. **The power vocabulary is extensible** so observability, approval
   powers, and agent-specific powers slot into the same chain and lease
   schema (invariant 4).
6. **No rung makes one person the only path to progress** — the governing
   principle, enforced by construction: permissionless-from-the-owner fork,
   admin-set takeover, membership-derived viewership; the host lease binds
   only the act of spending another person's subscription, which is the
   principle's own boundary (nobody may be *made* a bottleneck — and
   nobody's wallet may be conscripted either).

## 6. Must-have coverage map

| Must-have | Where |
| --- | --- |
| #1 permanent goal at top | D1 slice 4 (first user-visible win) |
| #2 pills: sub-agents/processes/ports | Deferred **[DECIDED]** — D6 tripwire keeps the door open |
| #3 default me; invite + co-input while connected | D2: session capability ∩ host lease; relay enforcement is the release gate; quota-spend host-authorized and labeled |
| #4 fork to own machine / copy own session | D5 (slice 1 same-machine, slice 2 cross-machine) |
| #5 shared visibility; invite collaborators | Viewer-by-default **[DECIDED]** + D2 grants |

---

## 7. v3 addendum — review reconciliation, shipped work, document index

### 7.1 Independent idea review, reconciled

An unanchored review by gpt-5.6-sol (given only SESSION_VISION.md and the
brief; no plan, no decisions, no prior analyses) endorsed the center of
gravity — "the durable unit should be the work session, not the vendor
process" — and challenged the vision as too transcript-centric and too
casual about collaboration vs remote operational control.

**Adopted into execution posture** (no design change needed):

- **Attribution ≠ verification.** Provider-signed facts establish who
  claimed something, not that it is true. Rendering rule for all session
  surfaces: signed facts render as *claims by their author* ("provider X
  reported the tests passed"), never as verified outcomes. Costs wording,
  prevents a trust bug.
- **Semantic capability, not host telemetry.** Matches the NIP-ST precedent
  (§7.2) exactly; governs the eventual return of must-have #2.
- **Continuity levels.** The review's seven-level continuity ladder is the
  user-facing generalization of D4a's five-facts honesty rule; continuation
  UI copy should name the level ("continued from history only" vs
  "continued at a verified commit").

**New named design question** (assigned to the authority track, to be
answered before the invite UI ships): **durable record vs revocable
access.** What happens on membership loss, a secret in a transcript, or a
private→shared transition, given an immutable signed record. Ingredients
that already exist: NIP-09 deletion, project-ACL re-reveal machinery
(`delete_project_acl`), and the viewer-model composition in §7.2.

**[OPEN-BRIAN] — challenges to standing decisions.** Standing decisions
remain in force until Brian rules otherwise:

1. **Custody vs presence-gated co-input.** The review argues for explicit
   operational custody — one control-token holder per execution, visible
   consensual transfer — over simultaneous co-input with queue receipts.
   Affects the shape of the lease/queue bites (B-track A6), not A1–A5.
2. **A "Contribute" rung** (add context/direction without controlling
   execution) between viewer and operator.
3. **Goal wording.** "Permanent goal" vs visible revision history.
   Execution rule adopted regardless: the goal representation must not
   destroy history; only the UI promise is open.
4. **Fork knowledge cite-back** (lightweight citation of another session,
   no merge). Post-phase; the lineage pointer already enables it.

### 7.2 Shipped work reconciled (upstream, 2026-08-11 → 08-13)

**`feature/project-access`** (Andy): project-level ACL on kind:30621 —
`buzz-access: public|private` + invited members as `p` tags (owner
implicit, cap 256), **relay-authoritative and fail-closed** at every read
chokepoint (REQ/COUNT/search/fan-out/git) via `project_acl` tables and a
cached per-channel `ProjectGate`. Flat two-tier (owner + members), no
roles. Consequences for this plan:

- **Session visibility composes**: viewer-by-default (decision 6) means
  *channel membership ∩ project gate* — already relay-enforced. A session
  in a private project is already invite-only with zero new machinery,
  which answers the review's visibility-default critique natively: teams
  that want private-until-invited sessions put them in private projects.
- The invite picker (`PersonaShareRecipients` + `allowDirectPubkeyEntry`)
  is the component the grant/invite UI reuses.

**`feature/builtin-shell`** (Andy, NIP-ST): terminal observation that
**reuses the project ACL wholesale** rather than inventing grants —
addressable announce (30623, deliberately no cwd/no shell path) plus
ephemeral watch/frame kinds (24310/24311), relay-enforced at write,
fan-out, and read; observers structurally have no input path; frames only
trusted from the owner's own signature; freshness windows and per-pubkey
rate limits. Consequences:

- The relay enforcement bite (command binding) copies these patterns
  (`filter_fanout_by_access` branch, `*_hidden_from` read predicate,
  coordinate-gate cache, `ScopedRateLimiter`).
- Must-have #2's eventual shape has a shipped precedent: owner-controlled,
  opt-in, semantic-status-only observation.

**Not verifiable from here:** the relay git origin (lightyear) rejects
non-interactive auth (no Nostr key configured for `git-credential-nostr`).
If Andy's agents push only to the relay, that work is invisible to this
plan. **Brian: run `git fetch origin` interactively before kickoff.**

### 7.3 Document status index

| Document | Status |
| --- | --- |
| `SESSION_VISION.md` | **Product authority.** Current. |
| `SESSION_NEXT_PHASE_BRIEF.md` | Input snapshot for this phase. Its §3 [UNVERIFIED] items are promoted/refuted in §1 of this plan. |
| `SESSION_DESIGN_PHASE_PLAN.md` | **Design authority** for the phase (this document, v3). |
| `SESSION_EXECUTION_PLAN.md` | **Execution authority** — bites, proofs, orchestration rules. |
| `SESSION_PATH.md`, `SESSION_STEP4_DESIGN.md` | Historical: pre-phase roadmap/design; their Steps 1 and 4 shipped. |
| `SESSION_NATIVE_SUBSTRATE.md` | Historical reconnaissance input. |
| `SESSION_HANDOFF_SOL.md`, `SESSION_HANDOFF_SOL_2026-08-14.md` | Historical handoff records. |
| `coding-session-analysis.md` | Evergreen how-to (querying stored sessions). Current. |
| `remote-agents.md`, `docs/nips/NIP-CSL.md`, `NIP-CSC.md`, `NIP-ST.md` | Adjacent specs. Current; NIP-CSL/NIP-CSC gain amendments in this phase. |
