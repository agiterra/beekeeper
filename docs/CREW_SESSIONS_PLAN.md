# Crew sessions — the durable plan

_Created 2026-08-25. Owner: Brian (direction), the lead seat (this plan's
orchestrator). Status per slice lives in §7 and is the only part of this file
that changes routinely; findings still go into `docs/SESSION_STATE.md`._

## 0. What we are building, in one paragraph

A coding session in Bee Keeper is already a shared, signed, observable room
where several provider executions work while humans watch and steer. This plan
turns those executions into **a crew**: each seat is a specific agent identity
with a role, seats can address each other durably, a lead seat can bring in a
disposable builder or a different-family architect, and the human founder keeps
founder authority over all of it. It is the reincarnation of the amas crew
model on Bee Keeper's substrate — one bus (the relay), one record (signed
kinds 44220–44230), one CLI (`bee`) — and it deliberately leaves amas's memory
apparatus out until a crew has finished work nobody routed by hand.

Brian's words, carried from the amas handoff, are the acceptance bar:
*agents have the memory, excellent comms, the ability to communicate in chats
while also doing work on projects — teams of humans and agents working in
unison, not isolation.*

## 1. How this plan gets built (the operating model)

This is the working agreement from `AGENTS.md` made concrete, and it is
itself the first crew — run by hand until §4 S5 lets it run itself.

| seat | who | does | never does |
| --- | --- | --- | --- |
| **lead** | Claude Fable, this session | rules, writes briefs, reads reports and diffs, merges tier-0/1, updates §7 | writes feature code; reads a builder's exploration |
| **builder** | Sonnet/Opus 5 or Codex, one per lane, isolated worktree | implements a locked brief; self-verifies; reports raw facts | redesigns; touches files outside its lane; commits to `main` |
| **refuter** | a *different model family* from the builder | one pass over a tier-2 diff against the brief's named constraints; terminal verdict | re-argues a disposition; reviews tier-0/1 |
| **runner** | cheapest capable model | `just ci`, e2e, builds; reports exit codes and counts | reasons about the diff |
| **finalizer** | lead, or a builder it names | rebases lanes onto `main`, `git commit -s`, force-pushes the topic branch | anything else |

**Lane rules.** Every lane owns a file set written in its brief; two lanes
never own the same file. Lanes work in worktrees (`git worktree add`), rebase
onto local `main` with `--signoff`, and deliver a branch plus a report — they
do not merge. The lead reads the report and the diff, not the transcript.

**Tiers** (from amas `roles.yaml`, unchanged because it was measured):
tier-0/1 = tests, fixtures, docs, NIP text, TS/Rust types with no runtime
change → builder tests + lead hunk-level read → merge. Tier-2 = provider
runtime, custody/keys, relay ingest, durable state → builder → cross-family
refuter → lead merge. Applying tier-2 ceremony to a tier-0/1 change is a
violation, not diligence.

**Verdict vocabulary.** Refuter: `CONFIRMED: <inputs/state → wrong outcome>`
or `NOT-REFUTED`. Lead: `APPROVE`, `APPROVE-WITH-NOTES` (notes are record,
not conditions), `BLOCK: missing-input = <the one thing, and who fetches it>`.
"I have concerns" is not a verdict. Three rounds that reframe instead of
refine = one missing input; stop the lane and name it.

**Honesty rules.** `verified` means the command ran green on this host with a
count; a green that names no count is worthless. A report claim without
`file:line`, a SHA, or an exit code did not happen. A brief that conflicts
with reality is a report back, not a licence to improvise.

### 1.1 Brief template (the lead fills this; the builder treats it as law)

```
LANE <id> — <one line>
Tier: 0/1 | 2 — because it touches <what>.
Branch: <topic>/<lane>, worktree from local main @ <sha>.
Owns (exclusive): <paths>. Must not touch anything else; if you need to, STOP and report.
Problem, with evidence: <file:line / reproduced output>.
Design (LOCKED): <decisions, numbered>. Deviations need a written reason in the report.
Contract changes: <exact wire/type deltas, with the doc that must change>.
Tests you must add: <named>. Watch each fail before the fix where a defect is claimed.
Acceptance: <commands with expected counts / exit codes>.
Out of scope: <named temptations>.
Report format: §1.2.
```

### 1.2 Report template (the builder fills this; the lead reads only this)

```
Branch + HEAD SHA, rebased on main @ <sha>.
Files touched (each: added/modified, one line why).
Tests: names + counts, the command, exit code. Red-before-green: which test, what it said.
Deviations from the brief and why.
Residuals: what you could not verify on this host, named.
Anomalies: anything surprising, even if unrelated.
```

## 2. Where the seams are today (verified 2026-08-25, cited)

These are the facts each slice builds on. Re-verify before briefing — the
tree moves.

- **A turn command carries a `commandId`; the echo does not.** `SessionCommand::Turn { command_id, .. }` reaches the actor (`crates/buzz-session-provider/src/session.rs:234-243`) and `TurnStarted` (`:1099-1104`), but `begin_turn` → `user_prompt_item` (`transcript.rs:104`, `crates/buzz-core/src/coding_session_payload.rs:793`) drops it. The desktop therefore settles a pending turn by *matching prompt text* (`desktop/src/features/coding-sessions/lib/codingSessionPendingTurns.ts:210-214, 249`).
- **Turn receipts exist for exactly one outcome.** `lib.rs:1594-1607` publishes `LifecycleReceipt::failed(UNAUTHORIZED_OPERATOR)`; every other turn outcome — queued, started, dropped, ignored as stale/unknown/closed — publishes nothing. `ReceiptStatus` (`coding_session_payload.rs:90`) has no turn vocabulary. The publish queue fences on `(kind, semantic key)` and the receipt key is `…|<commandId>` (`lib.rs:2071`), so per-stage receipts need per-stage keys.
- **A queued turn is consumed before it runs and lives only in memory.** `on_turn` calls `state.consume_command` (`lib.rs:1641`) *then* delivers; inside a running turn the command joins an in-memory `VecDeque` (`session.rs:1002, 1168`); overflow past `SESSION_QUEUE_DEPTH = 8` becomes a visible `turn_dropped:queue_full` item (`lib.rs:2515-2532`). Restart replays from the persisted watermark (`lib.rs:729-746`) but replayed commands are ignored as `AlreadyConsumed`. **Crash between consume and run loses the turn.**
- **Steering is not on the wire.** `CodingSessionAction` has two variants (`crates/buzz-core/src/coding_session_command.rs:36-47`, `deny_unknown_fields`); `threadSteer` is `false` for every v1 runtime (`coding_session_payload.rs:349, 363`); the desktop queues client-side (SESSION_STATE item 53). `buzz-acp` does have native steer for channel agents (`crates/buzz-acp/src/pool.rs:316-351`).
- **An execution holds no identity.** The env fence strips every `BUZZ_*` and `NOSTR_PRIVATE_KEY` (`crates/buzz-session-provider/src/agent_fence.rs:53-73`; `EXEMPT` is empty) and the fenced briefing tells the agent so (`agent_fence.rs:~110`, pinned by a test at `:150`). Metadata `agent_ref` is hardcoded `None` (`lib.rs:2025`). Managed agents get `BUZZ_PRIVATE_KEY`/`BUZZ_RELAY_URL`/`BUZZ_AUTH_TAG` at spawn (`desktop/src-tauri/src/managed_agents/runtime.rs:530-531, 759`), keyed from `ManagedAgentRecord` (`types.rs:215`; nsec in the OS keyring).
- **Authority already admits any pubkey.** `granted_operators` is a set of raw hex (`state.rs:155-171`) applied only from relay-signed 40099 receipts; `operator_may_steer` (`commands.rs:451`) has no human/agent distinction. The relay enforces channel membership of the 44220 signer. The desktop picker hides agents (`desktop/src/features/agents/ui/PersonaShareRecipients.tsx:80-90`) but direct pubkey entry works; `bee sessions grant` requires lowercase hex (`crates/buzz-cli/src/commands/sessions.rs:1293`).
- **`bee` cannot write a turn.** `SessionsCmd` (`crates/buzz-cli/src/lib.rs:2234`) has list/transcript/tools/export/grant/revoke/roster. SDK builders for 44220/44221 already exist (`crates/buzz-sdk/src/builders.rs:2676, 2704`) with no CLI caller. `resolve_target` (`sessions.rs:952`) resolves an execution from a session id and reports generation ambiguity.
- **The read side of agent-to-agent already exists.** `buzz-session-context` MCP exposes `session_overview` / `session_history` / `search_session` (`crates/buzz-dev-mcp/src/lib.rs:155-177`) over a disk package the provider re-projects at each turn start (`lib.rs:2200-2300`); the projector already folds **every sibling execution** of the umbrella (`context_projector.rs:520-645, 783-844`) and each item carries its `target`. The package has no roster field, and the MCP attaches only when rehydration succeeds (`session.rs:835-878`, `lib.rs:1190-1300`: needs `session_ref` + `genesis_ref` + configured MCP command).
- **Roles do not exist; skills are declared but not wired.** `PersonaConfig.skills` (`crates/buzz-persona/src/persona.rs:123`) resolves (`pack.rs:249`) into `ResolvedPersona` marked "reserved, not yet wired"; nothing materializes pack skills into a workdir. `TeamRecord` carries `instructions` + `persona_ids` (`desktop/src-tauri/src/managed_agents/types.rs:763`). The only "role" vocabulary is ACL (`entityRoles.ts:13`).
- **Channel-agent delivery already queues.** `--dedup` defaults to `queue` and mid-turn handling to `steer` (`crates/buzz-acp/src/config.rs:344-359`); the desktop hardcodes both (`runtime.rs:679-680`). The `[Context]` block shape to mirror is `format_prompt` (`crates/buzz-acp/src/queue.rs:1568`) and `format_event_block` (`:1119`).

## 3. Design — the locked decisions

**D1. A participant is an actor seated on an execution.** Execution identity
stays `(signer, driver, instanceId, sessionId)`. A crew execution additionally
carries `actor` (agent pubkey) and `role` (a persona slug). The provider
publishes both in 44223 (`agent_ref` stops being always-null; `role` is a new
optional key). Human-created executions with no actor are unchanged.

**D2. The relay is the mailbox; a process is never the mailbox.** A turn is
not consumed until it *starts*. Restart replays unconsumed turns in order.
Overflow is a visible item, never silence. This is what "an agent's message is
never missed" means mechanically.

**D2 amended 2026-08-26 (lead ruling R1, accepted).** The guarantee Slice 2
delivers is "a turn accepted by the provider is never silently lost": if its
target generation is still live when the provider comes back, it runs exactly
once, in `(created_at, id)` order; if the generation is not live (the session
resumed as N+1, or has no live actor), it is answered with a durable
`turn_dropped`/`turn_refused` naming the reason, and it is the **sender's** job
to re-address it to the successor generation. "Eventually runs" was never
achievable under generation fencing and must not be claimed anywhere. The
re-addressing affordance — desktop "Resend to the resumed execution" from a
`NO_LIVE_EXECUTION`/`STALE_GENERATION` receipt, and `bee sessions send
--readdress` — is added to Slice 4's scope by this ruling.

**D3. Three delivery classes, chosen by the sender, executed by the provider.**
`boundary` (default: hold; inject when the current turn settles), `steer`
(native mid-turn injection where the adapter offers it; otherwise **downgrade
to boundary and say so in the receipt** — never cancel-and-merge for agent
traffic), `interrupt` (founder/lead only: cancel, then deliver). Carried as an
optional `deliver` field on `thread.turn.start`; absent means `boundary`.

**D4. Every turn command gets receipts, keyed per stage.** Stages:
`turn_queued`, `turn_started` (carries `turnId`), `turn_degraded` (steer →
boundary, with reason), `turn_refused` (with the existing error codes),
`turn_dropped` (queue full). The desktop settles a pending row on
`turn_started` by `commandId`, never by text. The `user_prompt` item carries
`commandId` so the transcript and the receipt agree.

**D5. One verb, every runtime.** Agent comms are `bee`, never a
provider-specific MCP tool: `bee sessions send`, `bee sessions create`,
`bee sessions inbox`, `bee sessions status`. Codex, Claude, Goose, and a shell
all speak the same command. `--to` accepts an execution target, a role slug
resolved within the umbrella, or `session`.

**D6. Key custody never crosses the wire.** A 44221 create names an `actor`
pubkey; the provider resolves that actor's key material host-locally from the
managed-agent store (the same keyring the desktop uses) and injects it past
the fence into the ACP process env. Unresolvable actor → receipt
`failed / ACTOR_UNAVAILABLE`; the execution is not created. The fenced
briefing text changes to say what the seat *does* hold.

**D7. Authority is the existing chain.** An agent seat may steer another
execution only if its pubkey holds `grant-operator` on the umbrella via 44228.
Founder authority (stop/resume/end) never moves to an agent. The human
founder can steer, interrupt, and end any seat. Another human's crew is
observable to me only as a `viewer` grant and never steerable — cross-crew
coordination happens lead-to-lead in a project channel, not inside either
crew's session.

**D8. A role is a persona pack plus a `role` slug.** Role packs ship in-repo
(`personas/roles/{lead,architect,builder,verifier,runner,poker}`), carrying
the brief/report/verdict conventions from §1, a skills directory materialized
into the seat's workdir `.agents/skills/` at launch, an MCP allowlist, and
default model/effort. Provider differences stay in the driver; role
differences stay in the pack. A `KIND_TEAM` whose personas carry roles is a
*crew*; launching it is genesis + N creates + grants in one signed sequence.

**D9. Every crew has a budget and a liveness answer.** An umbrella carries a
turn budget (setting, disclosed like the session ceiling); exceeding it
refuses further agent-originated turns with a receipt. `bee sessions status`
answers "dead or slow" from the lease (24223) and the last signed item, so a
lead never polls transcripts.

**D10. Memory stays out of scope** until S6's acceptance holds. The context
MCP and Pulse are the crew's shared memory for now; role craft lives in the
pack as prose. amas's own measurements (88% empty recall, reflection killed)
are the reason.

## 4. Slices

Dependency graph: S1 → S2; S1 → S3; S3 → S4; S4 → S5; S5 → S6. S2 and S3
share no files and run in parallel after S1.

### S1 — Truthful turns (commandId on the echo; receipts per stage)

Tier 2 (provider runtime + a receipt contract), but small. Three lanes.

- **Lane 1A (Rust, core + provider).** Owns `crates/buzz-core/src/coding_session_payload.rs`, `crates/buzz-session-provider/src/{transcript.rs,session.rs,lib.rs}` (turn paths only), `crates/buzz-sdk/src/builders.rs` (receipt builder signature). Add `command_id: Option<String>` to `user_prompt_item` and thread it from `run_turn`; add the D4 statuses to `ReceiptStatus` with `turn_id` and `reason` carried in the existing exact-key shape (extend the accepted shape list at `coding_session_payload.rs:477-488`); publish `turn_queued` at mailbox accept, `turn_started` at `run_turn`, `turn_dropped` where `TurnDropped` is raised, `turn_refused` for every `TurnDecision::Fail` **and** for `Ignore` reasons that name a target (stale generation, closed, unknown) — `NotAddressed`/`AlreadyConsumed`/`PastHorizon` stay silent. Semantic key `coding-session-lifecycle-receipt/v1|<commandId>|<status>`.
- **Lane 1B (desktop).** Owns `desktop/src/features/coding-sessions/lib/{codingSessionPendingTurns.ts,codingSessionIngressPayloads.ts,codingSessionTranscriptItemContract.ts,codingSessionTurnRefusal.ts}` and their tests, plus `ui/CodingSessionPendingTurns.tsx`. Parse the new statuses (exact-key discipline preserved); settle on `turn_started` by `commandId`, falling back to the text join only for echoes with no `commandId` (old providers) and saying so in the row; render `turn_queued` as "Queued by the provider" and `turn_degraded` verbatim.
- **Lane 1C (docs + CLI read side).** Owns `docs/nips/NIP-CSC.md`, `docs/nips/NIP-CST.md`, `docs/nips/NIP-CSL.md`, `crates/buzz-cli/src/commands/sessions.rs` (decoders only: `transcript`/`list` show receipt stages), `crates/buzz-cli/TESTING.md`.

Acceptance: provider unit tests name each receipt stage; relay-backed
`e2e_genesis`-style test proves a turn produces `turn_queued` + `turn_started`
receipts and a `user_prompt` whose `commandId` equals the command's;
`codingSessionPendingTurns.test.mjs` proves the same sentence sent twice
settles by id, not text; `just ci` green with counts. Refuter constraint: no
receipt for a command this provider was not addressed by (no cross-provider
chatter); exact-key decoders still reject a receipt with an extra key.

### S2 — The relay is the mailbox (durable boundary delivery; `deliver` class)

Tier 2. Two lanes.

- **Lane 2A (provider runtime).** Owns `crates/buzz-session-provider/src/{session.rs,lib.rs,state.rs,commands.rs}` (mailbox/consume/replay paths). Move `consume_command` from `on_turn` to the point a turn *starts*; make replay-after-restart re-queue unconsumed turns in `created_at` order; keep `SESSION_QUEUE_DEPTH` and the visible drop. Add `deliver` handling: `boundary` = today's queue; `steer` = if the descriptor's `threadSteer` is true, use the adapter's native path (wire it for `claude-agent-acp`/`goose` mirroring `buzz-acp/src/pool.rs:316-351`), else publish `turn_degraded` and queue; `interrupt` = authority check (founder or granted operator holding the umbrella's `lead` role once S3 lands; until then founder only), cancel, deliver next. Kill-test: SIGKILL the provider with two queued turns; on restart both are answered, once each, in the order they were sent. **"Answered", not "run" — see §7's S2 scope correction:** the killed process took its executions with it and a resume mints a generation the replayed command no longer addresses, so the answer is a terminal `turn_dropped`/`NO_LIVE_EXECUTION`. Re-addressing an owed turn to a new generation is deferred design, not part of this slice.
- **Lane 2B (core contract + desktop + docs).** Owns `crates/buzz-core/src/coding_session_command.rs`, `crates/buzz-relay/src/handlers/ingest.rs` (44220 envelope validator only), `desktop/src/features/coding-sessions/lib/codingSessionCommand.ts`, `ui/CodingSessionComposer.tsx` (the Queue action publishes immediately with `deliver: "boundary"` instead of holding the draft locally — item 53's local queue retires), `docs/nips/NIP-CSC.md`. `deliver` is optional with `#[serde(default)]`; `deny_unknown_fields` stays.

Acceptance (restated 2026-08-26 under ruling R1 — the original wording claimed
a guarantee generation fencing cannot give): the kill-test above with counts,
proving each queued turn is **answered exactly once, in sent order** — run if
its generation is still live, otherwise a durable `turn_dropped`/`turn_refused`
naming why. Not met and recorded as such in §7: the desktop e2e proving a
queued row survives app restart (`pendingTurns` is module-level, non-persistent
state — rebuilding rows from the relay on mount is deferred to S4). NIP-CSC
documents the three classes and the downgrade rule, and says plainly that
`steer` is boundary-only in this build (ruling R3). Refuter constraints: a
replayed turn is never run twice; a `steer` to a non-steering adapter never
cancels the running turn; the horizon still prunes.

### S3 — Agent seats (actor + role on an execution; custody; authority)

Tier 2 (keys). Three lanes.

- **Lane 3A (core + provider).** Owns `crates/buzz-core/src/coding_session_lifecycle_command.rs`, `crates/buzz-core/src/coding_session_payload.rs` (metadata shape), `crates/buzz-session-provider/src/{commands.rs,state.rs,agent_fence.rs,config.rs}` and the create path in `lib.rs`. `SessionCreate` gains optional `actor` (hex pubkey) and `role` (slug, ≤64 bytes, `[a-z0-9-]`); `CreatePlan`/`SessionRecord` carry them (`#[serde(default)]`); 44223 publishes `agent_ref = actor` and `role`. Add a host-local actor resolver (config: `BUZZ_CSP_ACTOR_STORE`, the managed-agent store the desktop already writes) and a **post-fence injection** of `BUZZ_PRIVATE_KEY`/`BUZZ_RELAY_URL`/`BUZZ_AUTH_TAG` for actor seats only; amend `FENCED_SESSION_BRIEFING` and its pinned test to state what the seat holds. Unresolvable actor → `failed / ACTOR_UNAVAILABLE`.
- **Lane 3B (desktop).** Owns `desktop/src/features/coding-sessions/lib/{codingSessionTypes.ts,codingSessionLabels.ts,codingSessionUmbrellaModel.ts,codingSessionUmbrellaComposerModel.ts,codingSessionRoster.ts}`, `ui/{CodingSessionHeader.tsx,CodingSessionAgentFocus.tsx,CodingSessionPeoplePopover.tsx,NewCodingSessionDialog.tsx}` and `desktop/src/features/agents/ui/PersonaShareRecipients.tsx` (an `allowAgents` prop; default unchanged). Execution label becomes `<actor display name> · <role>` with runtime/model demoted to the hover; the New Session dialog offers "Seat an agent" (managed agent + role) beside the provider picker; the People popover can grant an agent. Resolve `agent_ref` to a profile with `useUsersBatchQuery` like `CodingSessionFounderLine.tsx:20`.
- **Lane 3C (CLI + membership + docs).** Owns `crates/buzz-cli/src/commands/sessions.rs` (grant/revoke accept npub/hex via `PublicKey::parse`, per `messages.rs:231`), `desktop/src/features/coding-sessions/lib/providerChannelMembership.ts` (an actor seat is added to the session channel before the create, or the create is refused with a named reason), `docs/nips/NIP-CSL.md`, `docs/nips/NIP-CSAT.md`.

Acceptance: relay-backed e2e — create an execution with `actor` = a managed
agent; its 44223 carries `agent_ref`; the ACP child's env (captured via a test
adapter) holds the three vars and nothing else fenced; a 44220 signed by that
agent against a *sibling* is refused until a `grant-operator` receipt exists,
then accepted. Refuter constraints: no nsec on the wire or in any signed
event; the fence still strips everything for non-actor executions; a
`grant-operator` never confers stop/resume/end.

### S4 — Agents talk (`bee sessions send/create/inbox/status`; sibling roster; inbox at turn start)

Tier 1.5 for the CLI, tier 2 for the provider inbox. Three lanes.

- **Lane 4A (CLI).** Owns `crates/buzz-cli/src/commands/sessions.rs`, `crates/buzz-cli/src/lib.rs` (clap), `crates/buzz-cli/TESTING.md`. `send --channel --to <target|role|session> --deliver boundary|steer|interrupt [--reply-to <seq>] --content -` via `build_coding_session_command` + `sign_event` (auth-tag injected; the relay checks membership); `create --channel --session <ref> --role architect --driver codex-acp --model gpt-5.6-sol [--actor <pubkey>] --brief -` via `build_coding_session_lifecycle_command`; `inbox` lists receipts and prompts addressed to the caller's executions since a cursor; `status` folds lease + last signed item per execution into `live | quiet <age> | released | unknown`. Role resolution reads 44223 `role` within the umbrella and errors on ambiguity, never guesses.
- **Lane 4B (provider + context package).** Owns `crates/buzz-core/src/coding_session_context.rs`, `crates/buzz-session-provider/src/{context_projector.rs,context_store.rs}`, `crates/buzz-dev-mcp/src/session_context.rs`. Package gains a `roster` (per execution: target, actor, role, status, last signed seq/age); `session_overview` returns it; a `session_inbox` tool pages 44220s addressed to *this* execution with their receipt stage. The MCP attaches to every umbrella execution with a genesis, not only rehydrated ones (the bootstrap prefix distinguishes the two).
- **Lane 4C (prompting).** Owns `crates/buzz-session-provider/src/session.rs` (the boundary-delivery prompt block only) and `crates/buzz-acp/src/base_prompt.md` (a "Crew sessions" section). A boundary-delivered turn is rendered in the `[Context]` shape of `queue.rs:1568`: sender actor/role, delivery class, reply target, then the text — never as bare prompt text, so a seat can tell a sibling's message from its operator's.

**Added to S4 by ruling R1 (2026-08-26):** the re-addressing affordance for an
owed turn. A `turn_dropped`/`NO_LIVE_EXECUTION` or `turn_refused`/`STALE_GENERATION`
receipt is the point at which the sender learns their words did not run; the
desktop row gains "Resend to the resumed execution" and `bee sessions send`
gains `--readdress`, both of which re-sign the same text against the
*current* generation. Open questions the lane must answer, not assume: which
generation it resolves to, who resumed it, and what happens when the session is
closed.

Acceptance: two managed agents seated in one umbrella; agent A `bee sessions
send --to builder` while B is mid-turn → `turn_queued`, then `turn_started`
with the `[Context]` block visible in B's `user_prompt`; `bee sessions status`
reads `quiet 3m` for a seat whose provider was paused; TESTING.md blocks run
live against a local relay with counts. Refuter constraints: `--to role` never
resolves across umbrellas; inbox never exposes another execution's private
context; membership is checked by the relay, not trusted from the CLI.

### S5 — Role packs and crew launch

Tier 1.5 (packs), tier 2 (launch sequence). Three lanes.

- **Lane 5A (persona crate).** Owns `crates/buzz-persona/src/{persona.rs,pack.rs,resolve.rs,validate.rs}`, `desktop/src-tauri/src/managed_agents/{nest.rs,runtime.rs}` (skill materialization only). `PersonaConfig` gains `role`; `ResolvedPersona.skills` stops being reserved: at spawn, resolved skills are materialized into the seat's workdir `.agents/skills/<name>/SKILL.md` (mirroring `nest.rs:188-215`), never into a shared dir.
- **Lane 5B (the packs).** Owns `personas/roles/**` (new) and `docs/CREW_ROLES.md`. Six packs: `lead` (the five verbs; dispatch-first; never absorbs a dead dispatch), `architect` (one-sitting shape verdicts; `BLOCK: missing-input`), `builder` (brief is law; §1.2 report), `verifier` (one pass; `CONFIRMED`/`NOT-REFUTED`; different family from the builder is a launch check, not prose), `runner` (commands, counts, exit codes; no opinions), `poker` (drives the built app; screenshots; honesty bugs). Each pack's prompt is short; its craft is skills.
- **Lane 5C (desktop launch).** Owns `desktop/src/features/coding-sessions/ui/NewCodingSessionDialog.tsx` (a "Crew" tab), `lib/codingSessionGenesis.ts`, `lib/durableCodingSessionCreate.ts`, `desktop/src-tauri/src/managed_agents/team_events.rs` (teams may carry a `crew` block: ordered roles → persona ids, one `primary`). Launch = genesis → creates in order → `grant-operator` to the lead seat → first turn to the primary with the goal. Each step's receipt gates the next; a failed step leaves the earlier seats visible and the failure named.

Acceptance: from the desktop, pick a crew team, a repo, a goal; three seats
appear with actor·role labels; the lead's first turn contains the goal and the
roster; `bee sessions roster` shows the lead as operator; the verifier launch
is refused when its model family equals the builder's. Refuter constraints:
a partial launch never leaves an unlabelled execution; skills materialize per
workdir; no pack prompt exceeds the role's stated size.

### S6 — Budgets, liveness, and the first unrouted result

Tier 2 (budget refusal), then a live proof. Two lanes plus the lead.

- **Lane 6A (provider + settings).** Owns the umbrella turn budget: a setting beside the session ceiling (`desktop/src/features/settings/**` Sessions panel; `crates/buzz-session-provider/src/config.rs`), disclosed the same way; the provider refuses agent-originated turns past it with `turn_refused / BUDGET_EXHAUSTED` and lets founder turns through. `bee sessions status` gains the budget line.
- **Lane 6B (poker + runner packs, live).** The runner pack owns `just ci`/e2e as a background child whose completion re-invokes the seat (the item-54 failure class, solved by the seat model rather than the adapter: the runner's *turn* is the wait); the poker pack drives the built app through the E2E bridge.
- **Lead (by hand, once).** Pick one open tier-0/1 item from `SESSION_STATE.md`; launch the crew; do not route by hand; record in SESSION_STATE exactly what the crew did and what the lead did.

Acceptance — the plan's acceptance: a crew closes one ledger item with the
human founder observing and never relaying; the lead seat's transcript shows
it read reports, not transcripts; the refuter's verdict is in the record;
`just ci` was run by the runner and its counts are cited in the finalizer's
commit. Only then does D10 reopen.

## 5. Non-goals (deliberate)

- Memory tiers, curation, reflection (D10).
- Cross-crew steering; another human's seats are observe-only (D7).
- A new broker or any HTTP endpoint — every operation here is an existing or
  additive Nostr kind.
- Rewriting the umbrella UI; S3/S5 change labels and add a tab.
- Metered API spend: seats run under the founder's subscription on the
  founder's machine, as amas insisted.

## 6. Decisions still Brian's

1. **Agent-to-agent `steer` in v1** — allowed with the honest downgrade (the
   plan's default), or boundary-only until adapters expose native steering?
2. **Verifier family check** — a hard launch refusal (plan default, D8) or
   advisory like amas ended up with?
3. **Where actor seats run** — the founder's provider on the founder's
   machine (plan default, D6), or may a seat be created on another member's
   provider that advertises the actor?

## 7. Ledger

| slice | status | evidence |
| --- | --- | --- |
| S1 truthful turns | proven live 2026-08-26 (dev instance vs hive) · crew/s1-truthful-turns rebased on main@b2298102 · refuted same-family advisory (NOT-REFUTED ×2) · ready to land | Gate green, run to completion: `just ci` (every `ci:` recipe through `mobile-test`, log `/tmp/crew-s1-ci-round3.log`, tree 829d93c4 = 5f7ce22d) — fmt/clippy/biome/web checks clean, Rust unit suites all passed, desktop `pnpm test` 6172 passed / 0 failed, Tauri suites all ok, mobile 1465 passed; `just test` (relay-backed, run by the lead on 4b49249b) exit 0, 12/12 sections, 67 suites ok, 0 failed, 223 tests in the workspace-integration section; `pnpm check:px-text` exit 0 (gate round 2). The overnight runner reported before `just ci` finished, which is why the first record read red; the log proves it completed. Two test-only hitchhikers ride the branch: `bdaa3e04` (pair-relay `test_cancellation_immediate` — a flake, passes on unpatched main) and `60ec9ca2` (the naming-settings test no longer reaches the macOS keychain, which hung the Tauri step in a worktree). Relay-backed e2e `e2e_coding_session_turn_receipts.rs` ran green once in lane 1A against a local relay; the desktop app was not opened. Rebased onto main@af3b9b66 (Andy's 2026-08-26 shelf fix) per INTEGRATION.md § Landing a topic branch and re-gated on that base: `just ci` exit 0 (desktop 6178/0, mobile 1465, 33 Rust/Tauri suites ok, log `/tmp/crew-s1-regate.log`); `just test` exit 0 (12 sections, 2132 passed, 0 failed in the integration run); pushed with `--force-with-lease`. Rebased again onto main@b2298102 (the full-screen UX pass, landed 2026-08-26) — 13 code commits replayed clean, one docs conflict resolved (ledger item renumbered 53→58) — and re-gated there: `just ci` exit 0 (desktop 6209/0, mobile 1465, 100 Rust/Tauri suites ok, log `/tmp/s1-regate2.log`); `just test` exit 0 (2134 passed, 0 failed). Dev instance on this tree opened for Brian's live look. Live 2026-08-26: session "S1 receipts test" (channel 2a9c83e7…, codex-acp gpt-5.6-sol) — 6 turns, every user_prompt carries commandId, 13 receipts (created + queued→started per turn), duplicate-sentence settle proven on screen; create-embedded first turn, turn_dropped, turn_refused not exercised live. |
| S2 relay is the mailbox | built `crew/s2-s6`@`ecb16a52`, round-3 triage applied on `crew/lane-2F` · refuted contract & runtime correctness=CONFIRMED, test honesty and evidence=CONFIRMED (same-family, advisory) · lead rulings R1–R5 recorded below · gate: per-crate counts only, no full `just ci`/`just test` at this head | **Ruling R5 — every `file:line` below was re-resolved at `crew/lane-2F`'s tip; a citation that does not resolve at the recorded head did not happen.** **Gate (ruling R4).** No full `just ci` / `just test` counts exist for this head. What ran green here, explicitly: `cargo test -p buzz-core -p buzz-session-provider -p buzz-sdk -p buzz-relay --lib` (counts in the lane 2F report), `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, desktop `pnpm test` and `pnpm check:px-text`. **Evidence correction (finding evidence-5, CONFIRMED).** `just ci` (`justfile:379`) runs `test-unit` (`justfile:427`), which enumerates buzz-core, buzz-auth, buzz-voice, buzz-cli, buzz-db --lib, buzz-conformance, buzz-push-gateway, buzz-backend-kubernetes and buzz-agent --lib — **no buzz-session-provider, no buzz-sdk, no buzz-relay**; `scripts/run-tests.sh:78-146` adds only buzz-db and buzz-auth `--test '*'` plus a workspace `--test *`, which reaches no lib tests. So the earlier "101 `test result: ok` lines from `just ci` at `3c8f2a15`" contained **zero** provider tests and never ran the buzz-relay ingest pin, and that green predates every round-2 and round-3 fix. Only `.woodpecker/gate.yml:138` (`cargo test --workspace --exclude buzz-agent`) runs them, and `gate.yml:139` skips `demo_join_forwarded_arm_round_trips_echo` by name — which also retires lane 2A's `mesh_demo` anomaly, since neither gate can be reddened by it. **Any claim about buzz-session-provider, buzz-sdk or buzz-relay must cite an explicit `cargo test -p <crate> --lib` run or `.woodpecker/gate.yml:138`; `just ci green` is not evidence for those crates.** Round-2 counts that stand: `cargo test -p buzz-session-provider` exit 0 (277 lib + 2 integration passed, 0 failed), desktop `coding-sessions` 857 passed / 0 failed, `tsc --noEmit` exit 0, and the relay-backed `RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_coding_session_delivery_classes -- --ignored` exit 0, 1 passed / 0 failed. **Ruling R1 (findings contract-2, evidence-2) — ACCEPTED as an amendment to D2, written into §3 as "D2 amended 2026-08-26" and into the §4 S2 acceptance.** The guarantee Slice 2 delivers is "a turn accepted by the provider is never silently lost": if its target generation is still live when the provider comes back, it runs exactly once, in `(created_at, id)` order; if the generation is not live (the session resumed as N+1, or has no live actor), it is answered with a durable `turn_dropped`/`turn_refused` naming the reason, and it is the **sender's** job to re-address it to the successor generation. "Eventually runs" was never achievable under generation fencing and must not be claimed anywhere. Mechanism, re-resolved: `report_no_live_execution` records a durable refusal (`crates/buzz-session-provider/src/lib.rs:2337`), `decide_turn` then answers `AlreadyRefused` (`commands.rs:500`), `session.resume` mints generation N+1 (`lib.rs:1657-1660`) and the fence refuses the replayed generation-N command (`commands.rs:516-521`). The briefed acceptance "both queued turns run exactly once in order" was **NOT met** and is replaced by "answered exactly once, in sent order"; `lib.rs:8371` asserts two `turn_dropped` receipts, not two runs. Owed follow-up, now in **Slice 4's** scope by this ruling: re-addressing an owed turn to a resumed generation — desktop "Resend to the resumed execution" from a `NO_LIVE_EXECUTION`/`STALE_GENERATION` receipt, and `bee sessions send --readdress` (whose generation, who resumed, what if the session is closed). **Ruling R3 (finding evidence-4) — steer is DEFERRED, not delivered.** `NATIVE_STEER_DELIVERABLE` is `false` (`crates/buzz-session-provider/src/session.rs:78`) and `metadata_for` AND-gates the per-execution witness with it (`lib.rs:2759-2762`), so `capabilities.threadSteer` is false for **every** execution; the desktop reads that field (`CodingSessionWorkspace.tsx:710`, `CodingSessionUmbrellaComposer.tsx:310`) and therefore never sends `deliver: "steer"` (`CodingSessionComposer.tsx:295`). No `turn_degraded`/`STEER_UNSUPPORTED` row can be produced by this build; all steer/degrade evidence is unit-only with a hand-injected capability (`lib.rs:8740`, `lib.rs:8964`, desktop `canSteer: true`). The record, NIP-CSC and this row say **"steer: boundary-only in this build; native injection deferred until an adapter advertises it"** — never "steer shipped". When the const flips, the two `const { assert!(!NATIVE_STEER_DELIVERABLE) }` tripwires force those tests to be rewritten and the rewrite owes an end-to-end steer, plus a `pub use pool::{SteerAck, SteerError, SteerRequest};` re-export (`mod pool` is private at `crates/buzz-acp/src/lib.rs:13`). **Ruling R2 (finding contract-3) — ACCEPTED contract delta.** Three receipt codes that did not exist extend contract E's "an existing code": `NO_LIVE_EXECUTION` (`crates/buzz-core/src/coding_session_payload.rs:79`), `NO_TURN_IN_FLIGHT` (`coding_session_payload.rs:88`) and `QUEUE_FULL_TURN_KEPT` (`crates/buzz-session-provider/src/lib.rs:159`). They are the honest answers — `UNKNOWN_TARGET` and `SESSION_CLOSED` would both be false statements about a live execution with nothing running — and they are legal only because the same change opened the code list (`coding_session_payload.rs:510-514`: any nonblank, control-free, ≤64-byte code). The ≤64-byte `MAX_RECEIPT_ERROR_CODE_BYTES` bound (`coding_session_payload.rs:97`) is now written into `docs/nips/NIP-CSL.md` together with the divergence it exists to close: the desktop reader still bounds the field at 256 (`codingSessionIngressPayloads.ts:35`), so a 65-byte code would render in the desktop and be rejected as malformed by `bee sessions`. Noted, not fixed: no client can infer finality from the status alone — `turn_dropped` is terminal for `QUEUE_FULL`, `QUEUE_FULL_TURN_KEPT` and `NO_LIVE_EXECUTION` with nothing in the status saying so; a wire-shape question for a later slice, no key added. **Ruling R6 — fix-now triage APPLIED (lane 2F, four commits).** `mailbox-1` (CONFIRMED, the one runtime defect, D2 violation): the replay-hold branch pushed the `HeldCommand` and then wrote `watermark_ceiling` as a watermark, so the first held command of a burst advanced the channel floor past older turns the relay had not served yet and a death inside the 1.5 s window lost them with no receipt; the write is gone (`lib.rs:1081` pushes and returns), pinned red first by `a_replay_window_holds_the_watermark_against_an_older_turn_behind_a_newer_one` (`lib.rs:9417`), which failed 0 passed / 1 failed before the fix. `evidence-3` (CONFIRMED): `deliver_held_commands` (`lib.rs:988`) drained `replay.held` with `mem::take` and `?` out of the loop dropped the untried remainder on the floor; it now pops one at a time (`lib.rs:1015`) and hands the failed command and everything behind it back before returning `Err`, pinned red first by `a_failed_held_delivery_hands_the_rest_of_the_burst_back` (`lib.rs:9672`). `evidence-1` (CONFIRMED, honesty half): the two replay tests' doc comments claimed to close the run-loop gap they do not reach — corrected, and `the_run_loop_still_calls_both_replay_entry_points` (`lib.rs:9756`) is an explicit source-level pin on the `sleep_for(replay_delay)` arm (`lib.rs:341`) and the reconnect arm (`lib.rs:309`), verified red by deleting the replay arm. Driving `run_with`'s `select!` from a test remains deferred: it connects a live `HarnessRelay` first. `evidence-6` (CONFIRMED): the self-comparison in `codingSessionIngressPayloads.test.mjs` is now a literal expected key (which caught that structured fields join with no separator), and the superseded `ui/pendingTurnCaption.test.mjs`, which called `describePendingCodingSessionTurn` with one argument after it grew a required `ageMs`, is folded into `ui/CodingSessionPendingTurns.test.mjs`. `evidence-8` (CONFIRMED): the outbox paragraph moved back onto the publish arm the new replay arm had displaced, `NIP-CSC.md` now says "Three consequences bind providers" over its three-item list, and every cite in this row is re-resolved per R5. **Unmet briefed acceptance still open (finding evidence-7).** "A desktop e2e proves a turn sent mid-turn appears as a queued row that survives app restart" was **NOT met**: `pendingTurns` is module-level, non-persistent state (`desktop/src/features/coding-sessions/lib/codingSessionPendingTurns.ts:14-22`), so the row vanishes on reload while the 44220 and its `turn_queued` receipt stay signed on the relay. Rebuilding held rows on mount (a relay query for this pubkey's 44220s joined against latest turn receipts and reconciled with `user_prompt` echoes) is new design, deferred to S4 alongside R1's re-addressing affordance. **Deferred triage, one clause each:** `contract-1` — native mid-turn steer injection re-scoped to a later slice (needs the buzz-acp `mod pool` re-export, a steer channel through the prompt read loop, a `turn_started`/`user_prompt{steered:true,commandId}` publish path, flipping `session.rs:78`, and an end-to-end steer as acceptance); `steer-2` — the degrade keyed on `deliver == Steer && !steer_injected` (`lib.rs:2061-2069`) would mislabel a steer to an idle-but-live execution; dead code today, deferred into the native-steer follow-up with the requirement that it be gated on the same open-turn predicate the interrupt arm uses (`lib.rs:2082-2090`); `authority-1` — `operator_owns_session` (`commands.rs:581`) returns true for every signer on a legacy ungoverned record, so any channel member may publish `deliver:"interrupt"`; deferred to S3/D7 as a named input (fail closed or keep the class open, pinned on a record whose `founder_pubkey` is `None`); `evidence-5` (deferred half) — adding buzz-session-provider, buzz-sdk and buzz-relay to `just test-unit` / `run-tests.sh` is repo-wide tooling that lengthens every pre-push, carried as its own ledger item. **Residuals from the lanes:** the 1.5 s `REPLAY_REORDER_WINDOW` is a wall-clock guess, not a measurement; the rate-gated late resubscribe (`resubscribe_retry`, `crates/buzz-acp/src/relay.rs`) reopens a channel with no signal reaching this crate, so that path has no reorder window; the first turn embedded in a 44221 create is still consumed with the create, so a death between the create receipt and that turn starting still loses it; `DeliverError::Gone` is proven by a direct call to `report_undelivered_turn`, not end to end; `interrupt_delivered` is decoded, stored and excluded from generation folds but no surface watches it, so an interrupt still gives the operator no signed confirmation on screen; contract A's "validate() rejects any other deliver string" is implemented one layer earlier as a closed serde enum (rejected at decode); lane 2A followed its brief over plan §4 in editing `coding_session_command.rs` and `ingest.rs` (test-only there), which §4 assigns to 2B; and after an `evidence-3` hand-back the held commands sit until the channel's window reopens (reconnect) or the provider restarts — the floor stays below them, so nothing is lost, but nothing retries inside the process either. |
| S3 agent seats | not started | blocked by S2 |
| S4 agents talk | not started | blocked by S2 |
| S5 role packs + crew launch | not started | blocked by S2 |
| S6 budgets + first unrouted result | not started | blocked by S2 |

Status vocabulary: `not started` · `briefed <date>` · `built <branch@sha>` ·
`refuted NOT-REFUTED|CONFIRMED <what>` · `merged <sha>` · `proven live <date>`.
A slice is `proven live` only when its acceptance ran against a relay with
counts recorded here.
