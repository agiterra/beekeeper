# Team Wake Durability — Specification v3.1

Status: SPEC — coherent; supersedes v3 in full.
Verdict of the v3.1 review: **V3.1 DESIGN COHERENT — IMPLEMENT.**
Scope unchanged: `crates/buzz-session-provider` only. No relay change, no new
event kinds, no model polling, no timestamp watermark, no probabilistic
membership structure.

---

## 0. What was wrong in v3 (statement of record)

v3's four-counterexample analysis and its core mechanisms (resolved ledger,
set-difference discovery, terminal queue, deterministic ids, signed-before-
publish) survive. Four v3 *spec* defects do not:

1. **"One persisted round-robin driving both scanning and processing" named a
   goal without defining an atomic scheduler action.** Two functions
   (`next_discovery_channel`, `next_pending`) each advancing the shared
   cursor satisfied the sentence and starved channels. The current tree
   already replaced them with one selection per tick
   (`run_one_team_wake_tick`, lib.rs:1353-1368; sole cursor mutator
   `next_tick_channel`, team_wake_store.rs:356-367, sole call site
   lib.rs:1363, sole tick site lib.rs:357). §2 makes that shape normative
   and closes the residual gaps.
2. **"The resolved ledger's bound is inherited from the partition refusal"
   was stated without the live path.** Live capture bypasses the partition
   query, so the sentence as written was false. §4 states the true bound
   (combined-set ≤ envelope, already enforced at team_wake_store.rs:260-263)
   and the honest behavior past it.
3. **Saturation had no lifecycle.** v3 said "explicit per-channel refusal"
   but not whether it persists, backs off, re-probes, or what live traffic
   does meanwhile. §3 defines it.
4. **v3's decisive test allowed "selected" to stand in for "published."**
   §6 requires observation at the publisher seam.

## 1. Durable schema (`buzz-provider-team-wake-intents/v3`)

One JSON file, `team-wake-intents.json`, atomic whole-file writes
(`atomic_write`), quarantine-on-corrupt/unknown-schema
(team_wake_store.rs:91-116, 165-178), poisoned-on-write-failure
(team_wake_store.rs:180-201). Those three behaviors are ratified unchanged.

```
Snapshot {
  schema: "buzz-provider-team-wake-intents/v3",
  channels: [ChannelState],            // ≤ 1_024
}
ChannelState {
  channel_ref: Uuid,
  resolved:  [event_id],               // permanent report ledger, no eviction
  admitted:  [SourceRef],              // report FIFO, ≤ 64
  in_flight: Option<WakeIntent>,       // quota = 1
  terminals: [WakeIntent],             // ≤ 256, provider-local, never refused
                                       //   while under the sanity cap
  refusal:   Option<ChannelRefusal>,   // NEW in v3.1 — §3/§4
}
ChannelRefusal {
  code: "partition_saturated" | "resolved_ledger_full",
  at_unix: u64,
  refused_live_reports: u32,           // explicit-loss counter — §3.4
  last_refused_event_id: Option<String>,
}
```

Changes from the current tree:

- **`rr_channel` leaves the durable snapshot.** The fairness invariant (§2
  F1) is per-rotation coverage, which a restart trivially preserves from a
  fresh cursor; persisting it buys one durable write every 2 s forever
  (team_wake_store.rs:363-364 today). The cursor becomes an in-memory field.
  Unknown fields are ignored on read, so no migration is needed.
- **`active_channel` is deleted** (team_wake_store.rs:87, 371-398, 436-442).
  `replace_first`/`defer_first`/`retire_first` become channel-explicit:
  `replace_in_flight(channel_ref, intent)`, `retire_in_flight(channel_ref)`.
  The implicit "whatever `pending_for_channel` last touched" temporal
  coupling is the same hidden-shared-state class that produced the v3
  scheduler bug; no store API may depend on call order.
- `pending_for_channel` persists only when it promotes; returning the
  existing in-flight intent unchanged must not write
  (today it always persists, team_wake_store.rs:396).
- `ChannelRefusal` added per §3.

## 2. Scheduler — exact state machine

Answers to the open questions first: **yes**, each tick selects one channel
exactly once and scans/processes only that channel; **no** persisted
cross-tick phase machine — both phases complete within the visit; separate
discovery/processing pointers would also be interference-free but require
two fairness proofs and give busy channels two visits per rotation — they
are different, not safer, and are rejected.

**S1 (selection).** Per runtime tick (lib.rs:114, 2 s), compute the candidate
set and select at most one channel:

```
discovery_need(c) = c ∈ subscribed ∧ c ∉ scanned ∧ ¬discovery_backoff(c) ∧ refusal(c) = None
processing_need(c) = has_work(c) ∧ ¬processing_backoff(c) ∧ refusal(c) = None
  where has_work = in_flight.is_some ∨ terminals ≠ ∅ ∨ admitted ≠ ∅
candidates = { c : discovery_need(c) ∨ processing_need(c) }
select: smallest UUID > cursor in sorted(candidates), else smallest overall
cursor := selected
```

This refines the current candidate set (lib.rs:1357-1362, which admits every
subscribed channel regardless of need — an idle 40-channel deployment gives
the one working channel 1/40 of ticks). Backoff-parked and refused channels
are not candidates; they cost **zero** ticks, not one.

**S2 (visit).** For the selected channel only:
`discover_team_wake_partition_for(c)` if `discovery_need(c)`, then
`process_team_wake_for(c)` if `processing_need(c)` — the same two calls as
today (lib.rs:1366-1367), now gated by need.

**Single-mutator rule (structural, not conventional).** Exactly one function
mutates the cursor (`next_tick_channel`); it is called from exactly one
place (`run_one_team_wake_tick`), which is called from exactly one tick arm
(lib.rs:357). Scan and process functions take `channel_ref` as a parameter
and have no access to the cursor. A test asserts by grep-parity or API
visibility (cursor field private, no other accessor) that no second mutator
can be added silently.

**F1 (fairness invariant).** In any window of consecutive ticks where the
candidate set is stable with N members, every candidate is selected exactly
once per N ticks. Corollary: a blocked channel (work that always defers)
consumes exactly one visit per rotation; a saturated channel consumes zero
(it is refused, not a candidate) after the visit that discovered the
saturation. This is the exact answer to "at most one fair scheduling
opportunity."

**F2 (progress invariant).** If any channel has processable work and no
backoff, some channel performs work within N ticks. Follows from F1 plus
need-gated candidacy.

## 3. Saturation semantics

Two failure classes, discriminated by **type**, not string matching. The
projection layer already distinguishes them
(`ContextProjectionError::Bound` vs `::Relay`, context_projector.rs:587-592);
`fetch_verified_snapshot` currently erases this via `.to_string()`
(team_wake.rs:994, 1001, 1060) and must instead surface the variant.

**3.1 Transient** (relay unreachable, malformed page, snapshot unprovable):
in-memory per-channel exponential backoff, 1 s → cap 600 s, keyed by stable
reason — the mechanism already built (lib.rs:1462-1525,
`TEAM_WAKE_BACKOFF_CAP` lib.rs:157). Retried forever. Lost on restart by
design (retrying once after restart is harmless).

**3.2 Structural** (`Bound` from any complete-partition query, or the ledger
envelope of §4): write the durable `ChannelRefusal` record, log **once** at
`error!` with fields `target: "csp::team_wake"`, `channel_ref` (UUID),
`code`, `pages`, `page_rows` — never per-tick re-logging (today a saturated
channel re-runs a 32-page query per backoff expiry forever,
lib.rs:1387-1404). While the record exists the channel is not a scheduler
candidate: no scans, no processing, zero tick consumption.

**3.3 Retry triggers.** Exactly two. (a) **Restart re-probe:** on the first
tick after process start, a refused channel is candidate for one discovery
probe; success clears the record, `Bound` re-affirms it at `debug!`. This is
one bounded query per channel per process lifetime and self-heals if relay
retention ever changes. (b) **Operator action:** deleting the `refusal`
object from the store file. The exact runbook is: **stop the provider**, edit
`team-wake-intents.json` and remove that channel's `refusal` object, then
restart the provider. Live deletion is unsupported and can be overwritten by
the running provider's next atomic whole-file write. No timer-based re-probe —
the partition cannot shrink under current retention, so a timer only burns
32-page queries.

**3.4 Live traffic while refused.** *Reports:* refused at capture with an
explicit record — increment `refused_live_reports`, set
`last_refused_event_id`, log at `error!` with `channel_ref` and
`source_id`. This is deliberate, disclosed loss, not silent loss: admitting
work that §4 proves can never be verified would misrepresent the channel as
functioning. *Terminals:* **always durably admitted** under the 256 sanity
cap (team_wake_store.rs:296-326 unchanged) and parked with
`last_reason = "channel_refused"` — they are provider-local facts and
evidence; they deliver if the refusal ever clears. This is the exact
report/terminal asymmetry: reports are relay-recoverable in principle and
refusable in honesty; terminals are neither recoverable nor refusable.

**3.5 Restart behavior.** The refusal record survives; behavior after
restart is identical except the single §3.3(a) probe. `scanned` remains
in-memory and empty at open — every non-refused channel gets one complete
scan per process lifetime, unchanged.

## 4. The ledger bound — exact semantics

**True statement of the bound.** While a channel is scannable, every id in
`resolved ∪ admitted ∪ in_flight` names an event in the channel's 44244
partition, and a complete-queryable partition holds ≤ 32 × 1,000 events
(context_projector.rs:118, 125). Therefore the **combined** set is bounded
by the envelope. The enforcement point is capture itself:
`report_capacity_available` refuses when the combined size reaches 32,000
(team_wake_store.rs:260-263, 276-285) — live path included. The v3
sentence "bounded by the partition refusal" is replaced by this.

**Impossibility, stated plainly.** Exact, bounded wake processing beyond
the envelope on one channel is impossible under this slice's constraints:

- exact dedupe forbids probabilistic membership (a false "resolved" is
  silent loss);
- no-timestamp-watermark forbids every horizon/recency compaction of the
  ledger (both future- and past-dated events defeat any such horizon);
- no-relay-change forbids server-side receipt order, tombstones, or
  compaction markers;
- and decisively: **verification itself dies at the envelope.**
  `fetch_verified_snapshot` requires the complete 44244 partition
  (team_wake.rs:1054-1066) and complete authority/receipt partitions
  (team_wake.rs:988-1001); past the envelope those queries refuse, so no
  wake — report *or* terminal — on that channel can ever again be verified,
  regardless of ledger size.

**Consequence.** The envelope is not a cache limit; it is the truth
boundary of the whole verification pipeline. Hence the only mathematically
honest bounded choice is the one §3.2 defines: the channel enters a durable,
named, operator-visible `resolved_ledger_full` / `partition_saturated`
refusal and is wake-dead as a unit. Yes — permanent (until operator action
or retention change) disablement at the bound is the only honest option; the
constraints anticipated this ("state it plainly and define the refusal").
Product framing: a team-session channel approaching 32,000 team
transactions has outlived the session model; the remedy is a new session.
Relay-side compaction is a future slice, out of scope here.

## 5. Driver — exact transitions and crash matrix

In-flight intent states (all durable in `WakeIntent` fields):

```
Unbound   (target=None)
Bound     (target, command_id=H(source,target,attempt), unsigned)
Signed    (signed_event persisted)
AwaitingProof (relay_accepted_at set)
→ Resolved | Rebound | HorizonRebound
```

**R1 — identity mutation ends the pass.** Any pass that mutates `target`,
`attempt`, or `command_id` persists and returns before signing or
publishing. **Initial `None → Some(target)` counts** — ratified as
implemented (lib.rs:1725-1737). Rationale: uniformity makes the invariant
mechanically checkable ("a pass that publishes mutated no identity field"),
at the cost of one 2 s tick per wake. A publish pass therefore always finds
identity durable at pass start and checks `command_outcome` /
`command_echoed` for that exact id against this pass's snapshot
(lib.rs:1743-1752) before touching the relay (lib.rs:1776-1807).

**R2 — rollover settlement.** On observing a changed lead target, first
check outcome/echo for the **old** `(command_id, target)`
(lib.rs:1701-1714): proven ⇒ retire (the old generation consumed the wake;
the new generation is not woken for it); unproven ⇒ clear identity, bump
attempt, persist, return (lib.rs:1716-1723). Disclosed caveat: outcome
evidence lives in a newest-first-trimmed inbox
(context_projector.rs:648), so a very stale settled delivery can be
unprovable ⇒ one redelivery to the new generation. At-least-once, bounded
at one duplicate per rollover; the consumer's durable command consumption
dedupes same-id resends.

**R3 — horizon rebump** (lib.rs:1758-1773): expired `relay_accepted_at` ⇒
attempt+1, new id, unsigned, persist, return. Next pass is a fresh R1
publish pass for the new id.

**R4 — signed-before-relay**: sign, persist via `replace_in_flight`, then
publish (lib.rs:1788-1797). Ratified.

Crash matrix (each row = a test in §6):

| Crash point | Recovery |
|---|---|
| before durable capture | Report: restart full scan set-difference re-admits. Terminal: `open_turn` still set ⇒ `recover()` re-synthesizes; deduped on `(caused_by_command_id, source_target)` (team_wake_store.rs:296-310, lib.rs:850). |
| after capture, before bind | intent durable `Unbound`; next visit binds. |
| after bind, before sign | identity durable; next visit is a publish pass for that id (checks outcome first — a prior incarnation's publish is found). |
| after sign, before publish | exact signed bytes resend; same event id, relay dedupes. |
| after publish, before persist of accepted_at | `observed_command_at` recovers it from the relay inbox (lib.rs:1754-1757); if aged out, R3 rebumps and the consumer's consumed-command fence absorbs the duplicate. |
| after proof, before retire | next pass re-proves outcome/echo ⇒ retires; `retire_in_flight` is idempotent. |
| mid-scan | admission is per-event atomic; `scanned` is in-memory; restart rescans; ledger/admitted dedupe makes the rescan idempotent. |
| mid-refusal-write | either the refusal persisted (channel parked) or it didn't (next visit re-discovers the same `Bound` and rewrites it). |

## 6. Test matrix

**T0 — decisive driver test** (extends the existing harness:
`spawn_test_relay_with_events` lib.rs:5538, real `RelayEventPublisher`,
scripted partitions). **"Published" is defined as a kind-44220 event
received by the fake relay's endpoint** — never inferred from store
selection. Scenario: four channels; A's partition scripted to return
`Bound` (saturated); B permanently blocked (fake relay never yields a lead
target for B, so every B pass defers); C and D healthy; 66 report sources
and one terminal; the terminal is captured on a channel whose `admitted`
FIFO is full; one past-dated and one future-dated report;
provider drop-and-reopen (a) mid-discovery, (b) immediately after a publish
reached the fake relay but failed before `accepted_at` could be persisted,
(c) after proof
visibility but before retire; a lead-generation change after a proven
old-target delivery; and a final full replay of every source through both
live and scan admission. Assertions: A consumes zero ticks after its
refusal lands and its refusal record names its UUID; B consumes exactly one
visit per rotation while C/D drain completely (F1); each C/D source and the
terminal appear **exactly once** in the fake relay's received 44220s per
command-id family; the rejected old-generation EVENT is retried with the
byte-exact same signed event id; both skew-dated reports exactly once; the rollover case
publishes nothing to the new generation; no identity-mutating pass
publishes in the same pass (R1, assert via publish-count per tick); the
final replay publishes nothing.

**T0-pressure — truth-ledger/publisher pressure complement.** Drive 5,121
sources across four channels through the production `WakeIntentStore` and
the real `RelayEventPublisher`/fake-WebSocket EVENT/OK seam. Restart after
the first publication and again after resolution 4,097; assert the endpoint
receives exactly 5,121 kind-44220 EVENTs whose identifier pointers equal the
source set, the durable ledgers contain 5,121 resolutions, and a final
reopen plus replay emits no EVENT.

This is deliberately a seam-composed pair rather than a misleading 5,121-
source full-driver claim. The bounded T0 measured 393 scheduler visits and
5,794 sequential complete-snapshot queries for 68 unique wake event ids in
51.52 seconds even with the test clock clearing all backoff. A linear lower
bound for 5,121 full verified driver deliveries is therefore more than
430,000 sequential queries and roughly 67 minutes; complete-partition
serialization/folding and the growing atomic ledger rewrite make the real
cost superlinear. Replacing those production queries with injected snapshot
state would stop testing the driver seam. T0 therefore proves verified
driver transitions and crash boundaries at bounded scale, while T0-pressure
crosses the historical 4,096 boundary through the real publisher seam.

Separate focused tests, each mapped to its invariant:

| Test | Pins |
|---|---|
| corrupt-bytes quarantine | §1 quarantine (exists: team_wake_store behavior) |
| unknown-schema quarantine | §1 |
| poisoned write (injected `atomic_write` failure) | §1 — every subsequent op errors; reopen recovers last good state |
| terminal sanity cap at 256 | §3.4 — explicit error, named log, no silent drop |
| channel-count refusal at 1,024 | team_wake_store.rs:214-223 — named log with UUID |
| complete-partition `Bound` ⇒ durable refusal, zero further queries | §3.2 (replaces the backoff-loop test at lib.rs:5692) |
| refusal log/record carries channel UUID + code + counts | §3.2 |
| live report on refused channel increments `refused_live_reports` + names the event id | §3.4 |
| terminal on refused channel parks durably, never drops | §3.4 |
| restart re-probe: refusal clears on success, re-affirms silently on `Bound` | §3.3 |
| old-target settlement retires without waking new generation | R2 |
| `None→Some` bind returns before publishing | R1 |
| horizon rebump returns before publishing | R1/R3 |
| scheduler: idle scanned channels are not candidates | S1 |
| scheduler: single cursor mutator (API-visibility assertion) | §2 |
| fairness: stable candidate set of N ⇒ each selected once per N ticks | F1 |

## 7. Requirements audit

No remaining inconsistency. The one genuine conflict — "no silent source
loss" vs "exact bounded state" vs "no timestamps / no probabilistic
structures / no relay change" — is resolved by reading "no *silent* loss"
strictly: past the truth envelope, loss of new report wakes is permitted
**only** as a durable, counted, named, per-event-logged refusal (§3.4),
and terminals are exempted from loss entirely. Everything else composes.

---

# Prompt for Codex Sol — v3.1 execution

You are executing spec v3.1 above in
`/Users/brian/Projects/beekeeper/beekeeper.worktrees/provider-team-wake`.
Read AGENTS.md and this file end-to-end first. The current uncommitted v3
implementation is the right skeleton — this is a correction pass, not a
rewrite. Do not regress the four v2 counterexamples' fixes.

Deltas, in order:

1. **Store** (`team_wake_store.rs`): delete `active_channel` and make
   in-flight APIs channel-explicit; move `rr_channel` to memory only;
   `pending_for_channel` persists only on promotion; add `ChannelRefusal`
   to `ChannelState` with §3 semantics.
2. **Error typing**: surface `ContextProjectionError::Bound` through
   `fetch_verified_snapshot` and the discovery query instead of
   stringifying (team_wake.rs:994, 1001, 1060) so the driver can
   distinguish §3.1 from §3.2.
3. **Driver** (`lib.rs`): need-gated candidate set per §2 S1; refusal
   lifecycle per §3 (durable record, log-once, zero-tick park, restart
   re-probe, live-report refusal counter, terminal parking). Keep R1-R4
   exactly as implemented — they are ratified.
4. **Tests** per §6: T0 at the driver/publisher seam plus the focused
   table. Each test's doc comment names the spec invariant it pins.
5. Update `PLAN.md`'s reference; record the outcome in
   `docs/SESSION_STATE.md` per its own rule.

Ground rules unchanged: hermit activated; topic branch, `git commit -s`;
no `unsafe`, no new `unwrap()`/`expect()` in production paths; doc comments
on new public API; 1,000-line ceiling — split, never bump.
Gates: `cargo test -p buzz-session-provider`, then `just ci`.
Report with `file:line` evidence per invariant. A completion report is not
evidence.
