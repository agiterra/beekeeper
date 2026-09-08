# Roles operator commissioning evidence

Status: implementation candidate in progress, September 8, 2026.
Scope: desktop evidence only. No execution permission changes or human gates.
Product authority: VISION_COLLABORATION.md, especially “Roles evolve with the project”.
Findings and completed validation belong in SESSION_STATE.md; this document defines
implementation, not a replacement ledger.

## Decision

Implement parity with the platform's existing recorded-timeline authorization
policy. Verify the accepted authority chain, then evaluate each lifecycle command
at its signed `created_at`, using the relay acceptance receipt's timestamp for
when each grant or revocation entered the timeline. This can confirm non-founder
commissioning under the policy the provider context projector already uses.

Do not claim immutable real-time admission or actual execution of role bytes.
The precise claim is: the reported provider was commissioned by a founder or an
operator authorized on the verified recorded timeline. Expose the distinction in
Technical details, without introducing another user choice or launch gate.

No producer, relay, database, CLI or provider change is required for this bounded
parity implementation. A stronger command-acceptance binding is a possible later
protocol improvement, not a prerequisite for this work.

## Current evidence and the assumption this corrects

- `desktop/src/features/roles/lib/rolePackProvenance.ts:573` invokes the shared
  coordination fold without commissioners. Its final chain check around line725
  confirms founder signers only; non-founder reports remain unavailable.
- `rolePackProvenanceQuery.ts:87` reads 44221 commands, 44224 provider receipts,
  44223 reports and 44226 genesis. It reads no 44228 transitions or 40099 relay
  authority receipts. `useRolePackProvenance.ts:61` omits those live kinds too.
- `desktop/src/features/coding-sessions/lib/codingSessionRoster.ts:329` already
  validates signed, receipt-backed, contiguous authority chains. It currently
  exposes current grants, seats and head, not the accepted transition timeline.
- `crates/buzz-session-provider/src/authority.rs:1` documents and implements the
  accepted receipt plus exact referenced transition trust boundary. Grants are
  not established by an arbitrary signed transition or a role label.
- Crucially, a historical projection already exists. Provider
  `context_projector.rs:2225` verifies the chain; around line2257 it records the
  **receipt's** `created_at` as `accepted_at`. `verify_lifecycle_command` around
  line2427 evaluates the command signer at the command's signed timestamp.
- `crates/buzz-core/src/coding_session_policy_fold.rs:58`,
  `signer_may_steer_at`, is the existing policy: founder always qualifies;
  otherwise iterate accepted links in chain order whose receipt timestamp is
  `<=` the record timestamp. Operator grants enable, viewer grants/revocations
  disable; seat grants/revocations do not confer steering authority.

Therefore “there is no historical authority mechanism” would be incorrect.
There is a usable timestamp-based policy; it is distinct from a server-recorded
atomic acceptance decision for each lifecycle command.

## What existing receipts prove

The relay's 40099 `coding_session_authority_transition_accepted` receipt binds
`genesisRef`, exact `acceptedEventId`, sequence, transition type, grantee and any
seat role (`crates/buzz-relay/src/handlers/side_effects.rs:1450`). It establishes
an accepted authority-chain link, with the relay's signed event timestamp.

A lifecycle 44224 is an outcome signed by the selected **provider**, not the
relay. `crates/buzz-core/src/coding_session_payload.rs:189` contains commandId,
status, target, error and optional turnId; no command event id or authority-head
binding. It cannot independently establish the issuing operator's authority.

Relay ingest explicitly applies base membership to create/resume and leaves
session authority enforcement to the provider (`handlers/ingest.rs:855–861`).
There is no existing relay-signed lifecycle-command admission receipt found in
these paths. A WebSocket OK is transport acknowledgement, not durable signed
operator-commissioning evidence. Successful provider receipt plus accepted
operator timeline is the bounded evidence join; neither replaces the other.

## Consumer contract

1. Keep project and channel boundaries. Resolve the exact genesis named by the
   create; verify sessionRef and channel agreement throughout every generation.
   Never derive founder or operator status from metadata, names, project ownership,
   agent ownership, presence or homeRole.
2. Obtain the active relay's trusted NIP-11 self key through the existing relay
   identity facility (`features/moderation/lib/relaySelf.ts`). Unknown/failed key
   reads leave operator proof unavailable. They must not erase otherwise complete
   historical founder proof. Include relay identity in proof cache dependency;
   preserve community reset and stale-result withdrawal.
3. Read 44228 and 40099 separately, with explicit kinds/channel filters. Admit
   authority receipts only from that relay key and the exact expected channel.
   Resolve each receipt's acceptedEventId; verify signature, exact envelope,
   genesis, sequence, type, grantee, optional role and previous accepted linkage.
   Deduplicate identical event IDs before counting conflicts.
4. Reuse the roster's strict parser/chain rules rather than adding another loose
   parser. Extract or expose a pure accepted-timeline helper if necessary. Return
   accepted links in sequence order with transition and receipt IDs and receipt
   timestamps, plus a complete/incomplete/conflicted disposition. Preserve all
   existing current-roster outputs and behavior.
5. Match the provider's legacy signer constraint: operator/viewer/revoke links
   are founder-signed; seat links retain the existing relay-accepted authority
   rules. Do not strengthen existing roster behavior globally merely to add the
   richer Roles projection. Roles may validate additional facts on its input.
6. Evaluate each command's own timestamp against the full verified timeline,
   in chain order, matching `signer_may_steer_at` exactly. Do not filter receipts
   by timestamp before verifying chain continuity. Do not use transition timestamps
   or sort authority by timestamps; sequence is the chain order.
7. Walk every create/resume link needed for the reported generation. Each signer
   must independently qualify as founder or accepted operator for that command.
   A grant for another session or channel contributes nothing. A current operator
   set, or union of project operators, must never substitute for this decision.
8. The shared coordination fold's default signer-not-provider rule currently
   rejects an otherwise authorized operator who is also the provider. If needed,
   add an optional, exact **event-ID** allow-set for externally verified
   commissioning, consumed only by this Roles caller. Populate it solely from the
   verified per-command authorization result. Authorize creates against their
   exact genesis before target uniqueness, so competing authorized claims stay
   visible. Only self-provider resumes need private lineage discovery; a
   provisional conflict must withhold a surviving target or descendant rather
   than disappear between passes. Ordinary histories use one fold. Preserve all default callers and
   conflict detection; do not filter competing commands/receipts out of the fold
   to make a preferred proof win. Final per-chain authorization still applies.
9. Missing/truncated authority reads suppress operator confirmation in their
   affected scope. Existing lifecycle/genesis incompleteness remains suppressive.
   Founder-only chains must not acquire a new dependency on authority history.
   Missing evidence remains unavailable; a definite contradictory binding stays
   disputed. Keep evidence visible with the concrete reason.
10. Arrival of relevant 44228 or relay authority 40099 invalidates affected proof.
    Ignore unrelated system messages. Failed, fetching and offline-paused reads
    retain the current rule withdrawing old confirmations until settled.

## Semantics and limits to disclose

A later grant cannot authorize a command with an earlier recorded timestamp;
a later revocation cannot erase a commission with an earlier recorded timestamp.
Same-second ties use the platform's existing inclusive `<=` rule. A command's
signed timestamp is supplied by its author; a relay receipt's timestamp is supplied
by the relay. Clock skew, backdating and same-second ordering are therefore limits
of this recorded-timeline policy, not proof of immutable real-world ordering.

Do not “fix” those limits here by silently adding timestamp rejection windows,
new authority gates, mandatory human reviews or requiring a new protocol for all
old work. Keep the existing provider policy and accurate product wording.

## Acceptance tests

Use real signed fixture bytes and the ordinary query/hook path where relevant.

- Historical founder create and founder resume retain their confirmed results
  even when relay identity or authority-history reads are unavailable.
- Accepted operator grant followed by create/resume confirms only the exact
  provider/report generation. Same key operator/provider works when independently
  authorized; two ungranted keys cannot certify one another.
- Grant accepted at101 does not authorize command at100. Grant at100 does
  authorize command at100, explicitly locking existing same-second policy.
- Grant at100, command at101, revoke at102 preserves the earlier confirmation;
  command at103 is unauthorized on that timeline. Viewer downgrade behaves as
  revoke. Grant-seat and a lead role alone never qualify.
- Transition timestamp predating command with receipt timestamp after it does
  not qualify. Nonmonotonic receipt timestamps are evaluated in chain order,
  consistent with the core function, after validating the entire chain.
- Wrong genesis/channel/relay signer; forged signature; mismatched accepted id,
  grantee/type/role; missing predecessor; conflicting sequence; duplicate receipts;
  failed and truncated reads cannot produce operator confirmation.
- Another project's operator grant in a shared transport channel grants nothing.
  Every resumed generation and every signer in its lineage is checked separately.
- Later arrival of grant/receipt changes a stale unavailable answer through normal
  invalidation, without polling a model. Revocation updates relevant timeline
  decisions. Community switches with identical event IDs never reuse authority.
- Bounded history recovery retains conflict completeness and disclosed exhaustion.
  A recovered selected proof alone does not clear a channel-wide truncation claim.
- Browser regression shows founder and operator reports, a missing-proof row and
  a contradictory row; descriptions never claim pack bytes executed. Retain
  responsive pagination and warm-refetch busy behavior.

Validation: focused pure and mounted tests, typecheck/lint, complete desktop suite,
rebuilt Roles browser spec. Relay-backed read acceptance can use existing signed
fixtures on an isolated relay; no production grant or message mutation is needed.
Follow TESTING.md for full smoke before eventual UI landing.

## File ownership and parallel delivery

Authority/parser lane:
- `desktop/src/features/coding-sessions/lib/codingSessionRoster.ts` and tests;
  optional extracted pure authority-chain helper beside it.
- New `desktop/src/features/roles/lib/roleOperatorCommissioning.ts` and tests.

Provenance lane/root integration:
- `desktop/src/features/roles/lib/rolePackProvenance.ts` and tests.
- `rolePackProvenanceQuery.ts`, `useRolePackProvenance.ts`, their tests and
  `useProjectPacksView.ts`; coordinate these with root's history recovery lane.
- Optional exact-event commissioning input in
  `desktop/src/shared/coordination/sessionCoordinationTypes.ts`,
  `sessionCoordinationCommissioning.ts`, `sessionCoordinationFold.ts` and tests.
  Claim this small shared seam explicitly before editing.
- Minimal Roles Technical details wording and browser acceptance spec.

This can ship independently of Fable's CI managed continuation slice. Fable owns
core/CLI/provider and may need strict desktop command-decoder parity. The paths
above do not require changes to command/receipt wire decoders. Notify Fable of
shared coordination ownership; keep his decoder work separate. No Rust, migrations,
relay handlers or CI operation schema changes are necessary for this recommendation.

## Optional stronger future contract, not part of this slice

If the product later requires authority at actual command acceptance rather than
recorded publication time, existing evidence is insufficient. A provider-authored
receipt with an arbitrary old authority-head id would still not prove that head
was current. The minimal credible producer would be a relay-signed durable
commissioning decision binding the exact command event ID, channel, genesis,
session, action/provider and accepted authority-head sequence/id, frozen at first
command admission under the same authority serialization boundary used by
`buzz-db/src/event.rs:2741` for transitions. It must survive retries without
re-evaluating old commands against new grants, and preserve decisions across later
revocation. Resume must resolve its predecessor's exact genesis; its current wire
payload contains target/provider only (`coding_session_lifecycle_command.rs:288`).

That stronger work needs relay/database durability and concurrency tests, delivery
recovery, a core receipt contract and desktop consumer support. It is a distinct
protocol slice with explicit file claims and migration coordination, not something
to smuggle into a desktop evidence improvement or a reason to block this parity work.
