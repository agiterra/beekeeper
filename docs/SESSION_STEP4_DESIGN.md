# Step 4 design: one session, multiple provider executions

Companion to [SESSION_VISION.md](SESSION_VISION.md) and
[SESSION_PATH.md](SESSION_PATH.md). This is the concrete design for Step 4:
one user-facing session containing multiple provider executions — a Claude
execution and a Codex execution as co-participants in one surface and one
record, with agent-to-agent exchange visible to the user.

Code references are to the `feature/coding-sessions` worktree
(`/Users/briansweet/agiterra/BuzzForkV2-coding-sessions`) as surveyed
2026-08-12.

---

## Summary of decisions

1. **Umbrella identity is a client-minted `sessionRef` UUID**, carried as a
   new nullable payload field on the 44221 `session.create` action and echoed
   by the provider as a new *optional* key on 44223 metadata. New clients mint
   a `sessionRef` on **every** create — a single-execution session is an
   umbrella of one, so adding a second provider later is a pure join with no
   migration step. A later create carrying the same `sessionRef` **joins** the
   umbrella as a new execution (new `cs-target`). Generation semantics per
   execution are untouched.
2. **Transcript facts never merge.** The umbrella is a grouping over N
   per-(signer, target) fact streams. The surface interleaves at **turn-block
   granularity**: each block is a window into exactly one execution's own
   stream, attributed to its signer; items are never cross-ordered between
   executions.
3. **The conversation lane is kind:9 in the host channel**, tagged
   `["cs-session", "<sessionRef>"]`. The relay already permits this tag with
   zero changes. New clients suppress these from the channel timeline; old
   clients see them as ordinary attributable chat (graceful degradation).
4. **Operator authority:** the signer of a create is that execution's
   operator; the signer of the earliest accepted create bearing a
   `sessionRef` is the umbrella **founder**. v1: only the founder attaches
   executions and prompts them; everyone else is a Step-3 observer. Enforced
   client-side + by consumer rendering rules; the relay stays a validating
   store.
5. **Handoff v1 is operator-mediated and adds no wire schema.** "Send to
   ⟨execution B⟩" on any item of execution A prefills a 44220 to B with a
   quoted excerpt and a `buzz://` provenance link to A's signed 44225 fact.
   The lane renders it as a structured handoff chip; if recognition fails it
   degrades to a plain quoted prompt. Agent-initiated handoffs are explicitly
   deferred.
6. **Backend change is ~30 lines of plumbing, no orchestration.** The sidecar
   already runs N sessions across N runtimes; it only needs to persist
   `sessionRef` per session record and echo it into metadata. The relay needs
   exactly one trivially-additive decode relaxation (44221 accepts the new
   optional payload key). No tag shapes change anywhere.

Invariant check: single-provider sessions render exactly as today (umbrella
of one shows no umbrella chrome, no selector, no lane requirement); every
event remains signer-attributed; the relay never parses provider content or
routes anything.

---

## A. Umbrella identity on the wire

### `sessionRef` value

A lowercase UUID (v4), minted by the creating client. It is a *session*
identifier, deliberately distinct from every provider-runtime identifier:
provider `sessionId`s live inside `cs-target` and remain execution details.

### 44221 `session.create` — new nullable payload field

`crates/buzz-core/src/coding_session_lifecycle_command.rs` — the
`SessionCreate` action gains `session_ref: Option<String>`, following the
`projectRef` precedent (nullable, validated when present):

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "csc-9f2c…",
  "action": {
    "type": "session.create",
    "projectRef": "30621:<owner-hex>:amas-redux",
    "repoRef": null,
    "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    "providerInstanceRef": "codex-primary",
    "providerAuthorityPubkey": "ab…64hex",
    "model": null,
    "title": "Advance Buzz live sessions",
    "initialTurn": "Implement the plan Claude produced."
  }
}
```

Validation when present and non-null: canonical lowercase UUID (36 chars,
hyphenated, lowercase hex). `null` is legal and means "no umbrella claimed"
(pre-Step-4 semantics — an implicit umbrella of one, see §F).

**Decode discipline — where this differs from `projectRef`.** `projectRef`
was in the schema from v1, so its key is structurally required
(explicit-null). `sessionRef` is added to an already-deployed schema; signed
v1 events without the key exist and must stay valid forever. So
`require_exact_fields` for the action becomes: **exactly the 8-key v1 set, or
exactly the 9-key set including `sessionRef`** — nothing between, nothing
beyond. New producers always write the key (explicit null or a UUID); the
8-key form is accepted only as the historical form. This is the versioned
additive discipline NIP-CSL's explicit-null rule exists to protect: an
omitted key on a *new* event is still indistinguishable from truncation, so
new clients never omit it — but the decoder cannot reject the past.

**Relay impact.** The relay's
`validate_coding_session_lifecycle_command_envelope`
(`crates/buzz-relay/src/handlers/ingest.rs`) calls this same buzz-core
decoder; the relaxation ships in one place. Tag shape is untouched: still
exactly three two-field tags (`h`, `csl-v`, `csl-command`), still `csl1-1`.
No tag carries the sessionRef, so umbrella and non-umbrella creates produce
identically shaped envelopes — same property `projectRef` has. This satisfies
the "trivially additive and backward compatible" bar: every previously valid
event remains valid, every previously invalid event remains invalid.

### 44223 metadata — new *optional* key

`crates/buzz-core/src/coding_session_payload.rs` — `SessionMetadata` gains:

```rust
/// Umbrella session reference echoed from the create, when one was claimed.
#[serde(skip_serializing_if = "Option::is_none")]
pub session_ref: Option<String>,
```

Emitted **only when the create carried a non-null `sessionRef`** — never as
an explicit null. This deliberately uses the fork's existing *optional-key*
mechanism rather than the required-explicit-null mechanism: the frontend
metadata parser (`codingSessionIngressPayloads.ts`,
`parseBuzzCodingSessionMetadata`) already distinguishes required keys from
optional ones (`contextSummary`, `diffSummary`, `planSummary`). `sessionRef`
joins the optional list, validated as a bounded UUID when present.

```json
{
  "schema": "buzz-coding-session-metadata/v1",
  "session": { "driver": "codex-acp", "instanceId": "i-1", "sessionId": "…", "generation": 2 },
  "projectRef": null, "repoRef": null,
  "title": "Advance Buzz live sessions",
  "agentRef": null, "provider": "codex-primary", "runtime": "codex",
  "model": "gpt-5.3-codex", "status": "running", "branch": null,
  "capabilities": { "…": "unchanged" },
  "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"
}
```

The relay does not parse 44223 (size cap only) — zero relay impact.

### Join, generations, and authority of the claim

- **Join** = a later 44221 with the same `sessionRef`, any provider, any
  authority. The receipt/metadata flow is completely unchanged; the new
  execution gets its own `cs-target`, generation 1, its own fact stream.
- **New generation** of an existing execution: the provider persists
  `session_ref` in its `SessionRecord` and copies it forward on generation
  bump/recovery. Same sessionRef, same execution, next generation — exactly
  the projectRef behavior.
- **Authoritative membership claim** is the operator-signed 44221 (joined to
  its minted target via the 44224 receipt's `commandId`). The 44223 echo is a
  projection convenience so the catalog can group without replaying creates.
  If a provider's echo ever disagrees with the signed create, consumers trust
  the create and flag the record — count-and-render-neither is the CST
  discipline; here it is count-and-render-flagged, because hiding an
  execution someone is operating is worse than showing a disputed grouping.

### 44222, 44224, 44225 — unchanged

The catalog, receipts, and transcript envelope do not change at all.
Transcript facts remain keyed by (signer, exact target, seq); the umbrella
never appears in a 44225, which is what structurally guarantees constraint 1
— there is no field through which two executions' facts *could* merge.

---

## B. Discovery and projection (frontend)

### Grouping model

Two levels above today's flat per-generation catalog record:

- **Execution** = one provider runtime session across its generations.
  Identity: `(signerPubkey, driver, instanceId, sessionId)` — the target
  minus `generation`. The active generation is the highest one with events;
  prior generations stay reachable as collapsed history within the rail.
- **Umbrella** = all executions sharing a `sessionRef`, plus the conversation
  lane. Records with no sessionRef form an implicit umbrella of one, keyed
  `implicit:<executionKey>`.

### File-by-file

- `lib/codingSessionTypes.ts`
  - `CodingSessionCatalogRecord` gains `sessionRef: string | null`.
  - New: `CodingSessionExecution` (`executionKey`, `signerPubkey`,
    `activeGeneration: CodingSessionCatalogRecord`,
    `priorGenerations: CodingSessionCatalogRecord[]`, `operatorPubkey:
    string | null`).
  - New: `CodingSessionUmbrellaRecord` (`sessionRef: string | null`, `title`,
    `executions: CodingSessionExecution[]`, `founderPubkey: string | null`,
    `status` (derived: `running` if any execution running, else
    `waiting_for_input` if any waiting, else the latest execution's status),
    `lastEventAt`, `conflictCount`, `foreignAttachmentCount`).
- `lib/codingSessionIngressPayloads.ts` — add `sessionRef` to the metadata
  parser's `optional` list with a bounded-UUID check; add the field to
  `BuzzCodingSessionMetadataV1`.
- `useCodingSessionCatalog.ts` — one-line change in
  `mergeTrustedCodingSessionIngress`: `sessionRef: metadata?.sessionRef ??
  null` on the record. The catalog stays **flat**; every existing consumer
  (menu, screenshots, popout bootstrap) is untouched.
- New `lib/codingSessionUmbrellaModel.ts` (pure, tested):
  `groupCodingSessionCatalog(entries, creates?) → CodingSessionUmbrellaRecord[]`
  — grouping, generation collapse, derived status, founder resolution
  (earliest create/metadata `created_at`; tie-break lowest event id),
  foreign-attachment flagging (execution whose operator ≠ founder).
- `lib/codingSessionWorkspaceModel.ts` — `resolveCodingSessionWorkspace`
  returns `{ umbrella, focusedExecution }` for the routed generationId. The
  route (`/coding-sessions/$channelId/$generationId`) is unchanged; existing
  deep links, popout labels, and bootstraps keep working. An umbrella surface
  is just what the workspace renders when `umbrella.executions.length > 1`.
- `lib/codingSessionBootstrap.ts` — popout bootstrap for an umbrella retains
  raw events for **all** member executions (loop `rememberCodingSessionPopoutBootstrap`
  over executions, keyed by the routed generationId as today).
- `ui/CodingSessionWorkspace.tsx` — renders the umbrella timeline (below)
  when N>1; renders **exactly today's single-rail tree** when N=1. The
  umbrella header (participant chips, add-provider button) mounts only when
  N>1 or when the founder invokes "Add provider" — invariant 2 is a render
  branch, not a mode.
- `ui/NewCodingSessionScreen.tsx` / `lib/newCodingSessionModel.ts` /
  `ui/useNewCodingSessionCreate.ts` — mint `sessionRef` at draft time; the
  new "Add a provider to this session" entry point (from the umbrella header)
  reuses the umbrella's existing sessionRef instead of minting, and pins the
  channel. Everything else about the create flow (catalog pick, durable
  create, receipt wait) is identical.
- `lib/codingSessionLifecycleCommand.ts` — builder writes the `sessionRef`
  key (always present, explicit null allowed), validates UUID shape.

### The session screen: interleaved narrative, not tabs

Recommendation: **one time-ordered narrative lane interleaved at turn-block
granularity, with full per-execution rails as progressive detail.** Not tabs.

- The vision explicitly rules tabs out: *"not a tab strip with one
  independent chat per provider."* Tabs hide co-activity, and watching agents
  work together in one stream is the differentiator this step exists for.
- "Readable narrative + progressive detail" maps directly: the narrative lane
  shows each **turn block** (user_prompt → collapsed tool/plan/reasoning
  activity → result) as one attributable card — provider badge, model, signer
  identity, accent color per execution — interleaved with conversation
  messages by time. Expanding a block reveals the detailed item stream;
  "open rail" shows that execution's complete transcript, which is exactly
  today's single-session surface.
- **Constraint 1 is preserved structurally**: interleaving happens *between*
  blocks, never *within* them. A block's items come from one (signer, target)
  stream in `eventSeq` order; CST conflict counting stays per-stream and
  renders on the owning block/rail.
- Ordering: blocks sort by their first item's `timestamp`; ties break by
  `(signerPubkey, targetKey)` for determinism. Conversation messages sort by
  `created_at` (seconds → milliseconds). An in-flight block pins where it
  started and streams in place (matching how a chat message would).

New pure module `lib/codingSessionUmbrellaTimeline.ts`:
`buildUmbrellaTimeline(umbrella, conversationMessages) → UmbrellaTimelineEntry[]`
with `UmbrellaTimelineEntry = { kind: "turn-block"; executionKey; turnId;
items } | { kind: "conversation"; message } | { kind: "lifecycle";
executionKey; … }` (joins/exits/generation bumps render as system rows). It
consumes the per-execution transcripts the existing
`projectTrustedCodingSessionTranscriptsToTranscript` already produces —
`turnId` grouping exists in that model today.

---

## C. Conversation lane

### Wire form: kind:9 + `cs-session` tag

Session-scoped chat is an ordinary channel message in the host channel with
one extra tag:

```json
{
  "kind": 9,
  "content": "Codex, take the failing test Claude found and fix the fixture.",
  "tags": [
    ["h", "<channel-uuid>"],
    ["cs-session", "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"],
    ["p", "<agent-pubkey>"]
  ]
}
```

Why kind:9 and not a new kind:

- **Verified relay-legal today.** Kind:9 ingest
  (`crates/buzz-relay/src/handlers/ingest.rs`, `KIND_STREAM_MESSAGE` path)
  validates only link-preview and `imeta` tags; arbitrary additional tags
  pass through. Zero relay change, which honors constraint 5 exactly.
- **Agents already live on kind:9.** The ACP harness's sibling messaging
  (`is_owner_or_sibling`, `crates/buzz-acp/src/lib.rs:198`) and p-tag
  addressing operate on kind:9 channel traffic. A new kind would need harness
  changes before any agent could see the lane; kind:9 gets human↔agent and
  (post-`agentRef`-wiring) agent↔agent conversation for free.
- **Attributable degradation.** Old clients render lane messages as normal
  channel chat — signed, readable, correctly attributed, merely unscoped.
  A new kind would be invisible to them.

Reading: the client already holds the channel's kind:9 stream; the lane is a
tag-filtered projection of it. A dedicated
`{"kinds":[9],"#cs-session":[ref],"#h":[channel]}` subscription follows the
existing `#cs-target` narrowing precedent (verify the tag index covers it —
open question 3) and always carries explicit `kinds` (p-gate rule).

**Timeline suppression:** new clients exclude kind:9 events bearing a
`cs-session` tag from the channel timeline (the same boundary that already
excludes 442xx kinds from chat), and surface them only in the session lane.
Whether they count toward channel unread badges is open question 5 —
recommended: session-scoped badge, not channel unread.

Only umbrellas with a non-null `sessionRef` have a lane. Implicit
(pre-Step-4) sessions don't — their composer offers only the execution,
which is today's behavior.

### Composer addressing

The composer gets a **participant selector**: one entry per execution
(labelled by runtime + model, e.g. "Claude · opus-4-1", "Codex ·
gpt-5.3-codex") plus **"Session"**.

- Addressing an **execution** → today's path exactly: a 44220
  `thread.turn.start` against that execution's exact generation target,
  interrupt/steer controls per that execution's honest capabilities
  (invariant 8 — the controls shown are the selected execution's, not a
  union).
- Addressing the **Session** → a kind:9 `cs-session` message. Humans and
  observers read it immediately; agents act on it per the existing harness
  policy (p-tag/mention + `owner-only`) once executions carry managed-agent
  identity (deferred item D2).
- **Defaults:** N=1 → the execution, selector not rendered (single-provider
  sessions look exactly like today — invariant 2). N>1 → sticky
  last-addressed participant, initialized to the most recently active
  execution. Sending never silently broadcasts: prompting all executions at
  once is deliberately not offered in v1 (it multiplies cost and interleaves
  chaos; the operator can prompt twice).

---

## D. Agent-to-agent exchange v1: operator-mediated handoff

The honest minimal version keeps a human signature on every actuation while
making the exchange fully visible and attributable.

### Mechanism

1. Every rendered item in execution A's stream (assistant_text, result, plan)
   offers **"Send to ⟨execution B⟩"** (one entry per other member execution).
2. Selecting it opens the composer pre-addressed to B, prefilled with an
   editable provenance block:

   ```
   > From Claude · opus-4-1 (this session) — buzz://coding-session?channel=<uuid>&target=<cs-target-key>&seq=<eventSeq>
   > The failing test is fixtures/relay.rs:88; the fixture predates the
   > membership gate. Recommend regenerating it rather than patching.

   Take Claude's finding above and regenerate the fixture.
   ```

3. Send publishes an ordinary **44220 `thread.turn.start` to B, signed by
   the operator**. No schema change; the 44220 contract is byte-identical to
   today's. The `buzz://coding-session` deep link (a client-side convention
   like the existing `buzz://message` link, resolved through the trusted
   ingress store — never a fetch-and-trust) pins the provenance to A's exact
   signed 44225 fact.
4. **Rendering:** the umbrella timeline shows the resulting turn block on
   B's rail as always; additionally, the lane/narrative renders a **handoff
   chip** — "Brian handed Claude's result → Codex", with a jump link to the
   quoted fact — by recognizing the provenance block in the operator-signed
   prompt text. Recognition is presentation-only: if it fails (edited text,
   old client), the message renders as a plain prompt containing a visible
   quote. Nothing on the wire pretends to be structured provenance, so
   nothing can lie about it; the operator's signature covers exactly what was
   said, and the link points at what A actually signed.

Attribution stays exact at every hop: A's words are A's signed fact; the
selection and framing are the operator's signed 44220; B's response is B's
signed stream. No re-signing, no merging, no relay involvement.

### Why not a structured `sourceRef` field on 44220

It was considered (an optional additive payload field, relay-relaxed like
`sessionRef`). Rejected for v1: 44220 is decoded strictly by every deployed
sidecar, and a sidecar has no version/capability negotiation — a
new-field-bearing turn sent to an old sidecar is silently ignored (no
receipt, no transcript item), which is the worst failure mode for the most
interactive path in the product. The prose+deep-link form is degradation-safe
everywhere. Revisit alongside open question 1 (contract revision signaling);
if catalog-level contract advertisement lands, `sourceRef` is the natural
follow-up.

### Full autonomy — explicitly deferred

Agents prompting each other *without* the operator requires, in order:

1. **Identity:** wire `SessionMetadata.agent_ref` (hardcoded null today) so
   an execution maps to a managed-agent identity that can sign events at all.
2. **Authority model:** today the relay's 44220 gate is channel membership
   only, and the sidecar doesn't even see the command's signer
   (`on_turn(channel_id, created_at, content)` — no pubkey). Autonomy needs
   an explicit, operator-signed delegation grammar ("agent X may start turns
   on execution Y, budget Z"), signer plumbed into the provider's decision,
   and provider-side enforcement — because "member" is far too weak for
   agent-initiated actuation against someone's checkout.
3. **Loop prevention:** handoff-depth budget, per-umbrella turn budget,
   cooldowns, and a human circuit breaker — two agents politely thanking each
   other in an infinite loop is the default failure mode, and each iteration
   costs money.
4. **Cost governance** across two different subscriptions/owners.

None of this blocks the operator-mediated version, and the v1 UI affordance
is exactly the surface that later becomes "approve the handoff this agent
proposed" (WF-08 approvals, kinds 46010+, `WaitingForInput`). Deferred.

---

## Operator authority (who may do what)

| Action | v1 rule | Enforcement |
| --- | --- | --- |
| Found an umbrella (first create) | Any active channel member (today's create rule) | Relay membership gate (unchanged) |
| Attach an execution (create with existing sessionRef) | Umbrella founder only | Client preflight (attach UI absent for non-founders); consumer renders a non-founder attachment as a **flagged foreign execution** — own rail, attributed, marked, never merged |
| Prompt / interrupt an execution (44220) | That execution's operator (= its create's signer; founder in v1) | Client preflight (Step-3 operator/observer composer); relay membership gate unchanged |
| Speak in the conversation lane (kind:9) | Any member with channel write (it is chat) | Existing chat rules |
| Observe | Step 3: any member with read access | Existing ACL + subscription |

The relay deliberately learns nothing new (constraint 5). **Recommended
hardening, optional in v1:** plumb the event pubkey into the sidecar's
turn/lifecycle decisions and have the provider ignore 44220s whose signer is
not the recorded create signer (it witnessed the create, so it knows). Small
additive change to `handle_command_event`/`decide_turn`; noted, not required
for Step 4 to ship. Co-operator grants (a founder-signed event admitting
another member as operator) are the Step-5-adjacent follow-up.

---

## E. Backend changes (verified minimal)

The suspicion in the brief is confirmed: the sidecar already runs N sessions
across N runtimes per create (`decide_lifecycle` →
`config.runtime(instance_ref)` → per-session adapter spawn in
`SessionManager::create`). The umbrella is a payload/UI concept. Concretely:

- `crates/buzz-core/src/coding_session_lifecycle_command.rs` —
  `session_ref: Option<String>` on `SessionCreate`; 8-or-9-key
  `require_exact_fields`; UUID validation. (This *is* the relay change —
  ingest calls this decoder.)
- `crates/buzz-core/src/coding_session_payload.rs` — `session_ref` on
  `SessionMetadata`, `skip_serializing_if = Option::is_none`.
- `crates/buzz-session-provider` — `CreatePlan` and `SessionRecord` gain
  `session_ref: Option<String>`; `create_session` copies it in; the metadata
  builder (`lib.rs` ~651) emits it; generation-bump/recovery paths carry it
  forward. No changes to actors, routing, outbox, or fencing.
- Tauri supervisor, `BUZZ_CSP_RUNTIMES`, catalog builder, `buzz-acp`,
  `buzz-sdk` builders (44221 builder mirrors the new field): mechanical.
- Relay: nothing beyond the shared decoder. 44223 remains unparsed.

---

## F. Migration and mixed-version behavior

**Old events, new client** — no `sessionRef` anywhere → each execution is an
implicit umbrella of one, rendered exactly as today. No backfill, no
migration event, no re-signing.

**New events, old client:**

- **44223 with `sessionRef`:** old `parseBuzzCodingSessionMetadata` fails its
  exact-key check → metadata dropped. The catalog's two-sided discovery
  saves the session: it remains visible from transcripts alone (generic
  title, inferred status, ungrouped). Degraded but present and attributable.
  This is also why the provider emits the key **only when non-null**: old
  clients lose enrichment only for umbrella-claiming sessions, never for
  old-style ones.
- **Lane messages:** appear as ordinary channel chat. Readable, signed,
  correctly attributed; merely unscoped.
- **44220 to a new execution:** unchanged contract → old clients can prompt
  any execution they can see (subject to the same authority rules).

**New client, old *sidecar*** — the sharp edge. A sessionRef-bearing 44221
fails the old sidecar's strict decode → the create is ignored with **no
receipt**, surfacing only as the durable-create timeout. Same-machine skew
cannot happen (the supervisor bundles the sidecar with the desktop app), so
this bites only when creating against **another member's** older provider in
a shared channel. There is currently no contract-revision signal in the
catalog to gate on — this is open question 1. Accepted for v1 with the
existing timeout UX; deploy order (backend crates before frontend) makes the
window one release wide within one machine, and cross-machine skew is
documented.

**Rollout order:** buzz-core + sidecar + relay first (all accept old and new
forms), frontend second.

---

## Deferred items

1. Agent-initiated handoffs / autonomy (authority grammar, signer plumbing,
   loop prevention, cost governance) — §D.
2. `SessionMetadata.agent_ref` wiring so executions have managed-agent
   identity (prerequisite for agents seeing/joining the lane as themselves).
3. Co-operator grants; "continue on my machine" (Step 5) — a new execution
   joining the umbrella under a different operator, which will relax
   founder-only attachment behind an explicit grant.
4. Structured `sourceRef` on 44220 (blocked on contract-revision signaling).
5. Provider-side signer enforcement for 44220/44221 (recommended hardening).
6. Broadcast-to-all-executions composer mode.
7. Relay-side write gate tightening (only operator may 44220 a target) —
   Step 3's optional hardening, unchanged by this design.

---

## Open questions, ranked by risk

1. **No contract-revision negotiation for lifecycle payloads (highest).** A
   new client creating against an old remote sidecar gets silence, not a
   receipt. Do we add a contract-revision signal now (e.g. a new *optional*
   catalog content key, which old clients' strict catalog canonical-form
   check would itself reject — the same problem one level up), or accept
   timeout UX until a catalog v2 is warranted? Wrong answer risks: flagship
   demo of "add Codex from another machine" failing silently.
2. **Founder-only authority vs. the shared-session vision.** The vision's
   proof includes a teammate attaching *their* execution on *their*
   subscription. v1 founder-only blocks that story; the co-operator grant
   design (deferred 3) needs to land soon after, and its event shape should
   be sketched before v1 ships so the founder rule doesn't ossify.
3. **`#cs-session` tag-query support.** `#cs-target` narrowing implies
   arbitrary-tag indexing on the fork relay, but verify the index and p-gate
   interaction before relying on the dedicated lane subscription (fallback —
   client-side filter of the channel's existing kind:9 stream — is cheap and
   always works).
4. **Interleave readability under two concurrently streaming executions.**
   Block-pinned ordering may bury an old still-running block; may need an
   "active blocks float / sticky" presentation rule. Pure UI, iterable after
   dogfooding.
5. **Unread/notification semantics for lane messages** — session badge vs
   channel unread; and whether old-client users find un-suppressed lane chat
   noisy enough to warrant accelerating client upgrades.
