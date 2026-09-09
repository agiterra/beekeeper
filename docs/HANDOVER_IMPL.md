# Absent-participant handover — implementation contract (plan step 4)

Product authority: `VISION_COLLABORATION.md` §"Continue another participant's
work", `docs/SESSION_VISION.md` §continuity, `docs/COLLABORATIVE_WORKSPACE_PLAN.md`
step 4. Brief: `BEEKEEPER-FABLE-HANDOVER-BRIEF.md` (Desktop). This is an execution
contract; status lives in `docs/SESSION_STATE.md` (root's).

## 0. The story, in the mechanisms that exist

A founds an umbrella (44226), grants B `grant-operator` (44228, relay-accepted,
40099 receipt), and works in execution E_A on A's provider P_A. A's agent commits;
the seat hook pushes `refs/heads/wip/<role>/<hex8>` (relay-signed 30618 says where
it stands). A goes offline: P_A's lease (24223) lapses, so E_A reads
`unverified`. Nothing today records *who owns the work*, nothing fences E_A when
P_A returns, and nothing carries the task/decisions/artifacts/next action in a
durable record. B can already steer E_A **only while P_A is alive**; B's provider
P_B ignores commands addressed to E_A (`commands.rs:746`), and the resume cursor
never leaves P_A's disk (`state.rs:249`). So:

- **Claim** = a new 44228 chain link. Relay-serialized (`seq`/`prevAccepted`,
  stale-head refusal), relay-receipted, folded by every consumer. Racing claims
  resolve at the relay: one accepted head, the other refused by name.
- **Fence** = providers fold the claim and refuse turns, resumes and wakes on any
  execution that is not the claimed body, or from any operator who is not the
  claimant, with a durable named refusal.
- **Checkpoint / continuation** = new kind **44247** (`KIND_CODING_SESSION_HANDOVER`),
  structure-validated at the relay like 44246, standing decided by readers from
  the canonical folds.
- **Native continuation** = B steers E_A on P_A (existing path) when P_A is
  reachable and B's grant stands. **Reconstruction** = a `session.create` on P_B
  joining the same `sessionRef`/`genesisRef` (existing pure join,
  `coding_session_lifecycle_command.rs:122-140`), seeded from the checkpoint, on a
  checkout of the recovered artifacts. Two labelled outcomes, never conflated.
- **Retirement** (root's prerequisite) = an accepted whole-session deletion
  (kind 5 naming the immutable genesis) retires the umbrella on every provider
  before recovery republication, restaging or handover consumption.

No new registry, HTTP API, polling agent, or human approval step.

## 1. Kind 44228 additions (authority chain)

`crates/buzz-core/src/coding_session_authority_transition.rs`:

- `CodingSessionAuthorityTransitionType` gains `Takeover` (`"takeover"`) and
  `Transfer` (`"transfer"`) — the names the module already reserves at `:12-13`.
- Payload gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub body_pubkey: Option<String>`
  (camelCase `bodyPubkey`): the **provider authority pubkey** of the execution
  body the claimant will use. Required (64-hex) for `takeover` and `transfer`;
  must be absent for every other type (validation error otherwise). `role` must
  be absent for both.
- `grantee_pubkey` = the **claimant**: for `takeover` it must equal the signer
  (self-claim only); for `transfer` it names the new claimant and the signer must
  be the current claimant or the founder.

Relay acceptance (`crates/buzz-db/src/event.rs::insert_coding_session_authority_transition_event`),
in addition to the existing linkage rules:

- `takeover`: signer is the founder **or** holds a live `operator` grant at this
  point in the chain (`grants_before`). `granteePubkey == signer`. Refusal codes:
  reuse `SignerNotAuthorized`; a mismatched grantee is `SelfNomination`'s inverse
  — add `AuthorityTransitionRefusal::ClaimantNotSigner`.
- `transfer`: signer is the current claimant (folded from the chain: newest
  accepted takeover/transfer whose claimant still holds standing) or the founder;
  grantee must be founder or live operator; add `NoActiveClaim` when there is no
  claim to transfer.
- The 40099 receipt (`side_effects.rs::handle_coding_session_authority_transition_accepted`)
  carries `bodyPubkey` when present.

Canonical claim fold (one function, used by provider, CLI and Desktop twin):

```
state = NoClaim
for link in accepted chain (seq order):
  takeover/transfer → state = Active { claimant: grantee, body: bodyPubkey, acceptedEventId, seq }
  revoke(grantee == claimant) | grant-viewer(grantee == claimant)
                    → state = Voided { last: <that claim>, voidedBy: link id, seq }
  grant-operator(grantee == last claimant) while Voided → still Voided (a regrant never restores)
```

`ClaimState` has three variants and consumers must keep them apart: `NoClaim`
(no handover ever happened; existing rules apply), `Active(claim)`, and
`Voided { last, voided_by }` (a handover happened and its claimant lost
standing; **the fence stays up for everyone** until a fresh accepted
`takeover`/`transfer`). Only the founder or a live operator can make that fresh
claim, and that is the deliberate visible act the story requires.

**Scope decision (v1): the claim is umbrella-wide.** The chain is rooted at the
genesis, so one claim hands over the *whole session* — every execution and
every assignment under it. Sibling executions of the same umbrella are fenced
alongside the absent one, and every surface says "hand over this session",
never "move this slice". Exact per-execution scope is a later contract; the
composition tests sibling behaviour explicitly.

Rust: `crates/buzz-session-provider/src/authority.rs::fold_current_authority`
gains `claim: ClaimState`; `buzz-core` holds the rule in
`coding_session_authority_claim.rs` (pure, shared by DB/relay/CLI/provider). TS twin:
`desktop/src/features/coding-sessions/lib/codingSessionMissionAuthority.ts`
(exact-key set + `claim` output).

Deletion precedence: a chain whose genesis is retired (§5) yields **no** claim
and no grants for consumers that have witnessed the retirement.

## 2. Kind 44247 — `KIND_CODING_SESSION_HANDOVER`

Allocation: 44247 is the lowest unused, unreserved kind in this fork and in
`vanilla/main` (record the grep in the doc comment, per `kind.rs:797-805`).
Envelope: tags exactly `h` (channel UUID), `d` (= `sessionRef`), `csh-v` = `"csh1"`,
`csh-genesis` = `genesisRef`, `csh-type` = record type. Content ≤ 32 KiB. Relay
validates structure only (`validate_coding_session_handover_envelope` in
`ingest.rs`, membership + `MessagesWrite` like 44246). Regular (not replaceable).

Schema `buzz-coding-session-handover/v1`, payload
`{ schema, sessionRef, genesisRef, type, body }` with `deny_unknown_fields`.

### 2.1 `checkpoint` — what the work is, durably

```jsonc
{
  "task": "…",                       // accepted task, ≤ 4 KiB
  "assignmentRefs": ["<hex64>"],     // 44244 assignment ids, ≤ 16
  "decisions": [{ "eventId": "<hex64>", "summary": "…" }],   // ≤ 32, summaries ≤ 512 B
  "revision": {
    "repoRef": "<repo coordinate or null>",
    "baseSha": "<40|64 hex or null>",
    "headSha": "<40|64 hex or null>",
    "branch": "<string or null>",
    "dirty": true | false,            // uncommitted changes existed when written
    "preserved": "all" | "partial" | "none"   // of the dirty bytes, what the artifacts hold
  },
  "artifacts": [                      // ≤ 16
    { "kind": "wip-ref", "repoRef": "…", "ref": "refs/heads/wip/…", "sha": "…" },
    { "kind": "patch",   "repoRef": "…", "eventId": "<hex64 of a NIP-34 1617>", "baseSha": "…", "bytes": 1234 },
    { "kind": "blob",    "repoRef": "…", "hash": "<sha256 hex>", "baseSha": "…", "bytes": 123456 }   // Blossom, for a patch above the event limit
  ],
  "tests": [{ "name": "…", "command": "…", "outcome": "passed|failed|not-run" }],   // ≤ 32
  "unresolved": ["…"],               // ≤ 32 × 512 B
  "nextAction": "…",                 // ≤ 2 KiB
  "missing": ["…"]                   // local-only bytes the author could not preserve, ≤ 16 × 512 B
}
```

Never private reasoning. A checkpoint is the **author's** statement; readers
label it "Checkpoint by {author}". Standing (reader-decided): founder, live
operator, or an active seat of the umbrella at the checkpoint's time; anything
else is listed as `unauthorized` and never used for reconstruction.

### 2.2 `continuation` — what the claimant did

```jsonc
{
  "claimRef": "<hex64 accepted takeover/transfer event id>",
  "mode": "native-resume" | "reconstructed",
  "checkpointRef": "<hex64 or null>",       // the 44247 checkpoint used
  "target": { "driver", "instanceId", "sessionId", "generation" },  // the execution now carrying the work
  "recovered": ["wip-ref refs/heads/wip/… at 9a1c…", "patch e7de… applied"],
  "missing": ["uncommitted changes on A's machine (dirty=true, no patch)"],
  "note": "…"                                // ≤ 2 KiB
}
```

Standing: author must be the claimant named by `claimRef`, and `claimRef` must be
an accepted link of this genesis. A continuation whose claim was later voided
stays historical ("continued by B until …").

### 2.3 Canonical fold — `crates/buzz-core/src/coding_session_handover_fold.rs`

Input: verified 44247 events for one umbrella + the accepted 44228 chain context
(founder, grants over time, seats) + retirement flag. Output:

```
HandoverFold {
  checkpoints: [ { eventId, author, createdAt, standing: authorized|unauthorized, body } ]  // ascending
  latest_authorized_checkpoint: Option<eventId>
  continuations: [ { eventId, author, createdAt, claimRef, mode, target, standing } ]
  claim: Option<{ claimant, body, acceptedEventId, seq, since }>     // from §1 rule
  active_continuation: Option<eventId>   // newest authorized continuation whose claimRef == claim.acceptedEventId
  retired: bool
  excluded: [ { eventId, reason } ]
}
```

TS twin in `desktop/src/features/coding-sessions/lib/codingSessionHandoverFold.ts`
pinned to a Rust-written fixture (the 44246/44244 pattern).

## 3. Provider — fence, admission, retirement

`crates/buzz-session-provider` (Lane X). `SessionRecord` gains
`handover: ClaimState` (persisted; `NoClaim` serializes as absent so legacy
state files decode unchanged) and `retired: Option<Retirement>`
`{ deletion_event_id, receipt_event_id: Option, at }`, both persisted.

Fence rule, applied in `decide_turn_command` (after the generation check, before
`operator_may_steer`), in `decide_lifecycle` for `session.resume`, and before any
team wake is admitted (`team_wake` admission path):

```
if record.retired                       → refuse SESSION_RETIRED (durable refusal)
match record.handover:
  NoClaim                               → existing rules unchanged
  Voided { last, .. }                   → refuse HANDOVER_FENCED { voided: true, last claimant }
                                          for every turn/resume/wake, on every body
  Active(claim):
     claim.body_pubkey != this provider → refuse HANDOVER_FENCED { claimant, body }
     operator != claim.claimant         → refuse HANDOVER_FENCED   (turns/resumes;
                                          provider-minted wakes on the claimed body proceed)
```

Required regression: revoke the claimant, regrant the same pubkey, then send an
actual old-body turn → still `HANDOVER_FENCED`; a fresh `takeover` by the
founder lifts it. `record.handover` persists the full `ClaimState`.

`HANDOVER_FENCED` and `SESSION_RETIRED` are new refusal codes in
`crates/buzz-core/src/coding_session_payload.rs` (Lane K adds the consts; Lane X
uses them). Refusals are recorded through the existing durable refusal ledger
before publish. The fence is restart-safe: `recover` re-derives it from the
authority chain (existing backfill) before publishing any metadata; a fenced
record publishes metadata status `disconnected` with `handover` fields (§3.1).

Native restore (`native_restore.rs`) refuses fenced and retired records with the
same codes; `seat_requests.json` omits retired records and marks fenced ones so
the host does not re-stage a body that cannot act.

Claim consumption: the provider already folds 40099 receipts live and on
backfill; the `takeover`/`transfer` links update `record.handover` for every
local record of that genesis. Unknown transition types continue to fail closed.

### 3.1 Metadata disclosure

44223 metadata gains optional `handover: { claimant, bodyPubkey, acceptedEventId } | null`
(additive; TS decoder accepts present-or-absent). A returning P_A therefore
advertises E_A as `disconnected` **and fenced by B**; the desktop reads it from
the coordination fold without a second query.

### 3.2 Retirement (root's finding)

The relay already authorizes whole-session deletions (founder **or** project
owner, `side_effects.rs::authorize_coding_session_deletion`). To let providers
verify either without inventing authority, the relay emits a signed kind 40099
receipt `coding_session_deletion_accepted { genesisRef, sessionRef, deletionEventId, channelId }`
when it applies one (Lane K, same path as the authority receipt). A provider
retires the umbrella when **either** holds: (a) a relay-signed deletion receipt
(verified against the witnessed NIP-11 self key, like authority receipts) names
the record's immutable `genesis_ref`; or (b) a kind 5 event signed by the
record's `founder_pubkey` names that `genesis_ref` **and** an authenticated
exact-id read of the genesis returns zero rows (the relay applied it). Deletions
older than this receipt exist only in form (b); project-owner deletions before
the receipt shipped remain a stated limit. Then `record.retired` is
persisted, no metadata is published, seat requests and restaging skip it, and
every command answers `SESSION_RETIRED`. Consumed on `recover` **before** the
"stranded" loop republishes anything, and live: the channel subscription adds
kind 5. A missing or failed read, an unaccepted signed request, or free-text
content is never retirement authority. Nothing is republished or reconstructed
to make a deleted session resumable.

## 4. CLI — `bee sessions handover …` (Lane C)

`crates/buzz-cli/src/commands/sessions/handover.rs` (+ `handover_git.rs`,
`handover_tests.rs`), variants on `SessionsCmd`:

- `handover checkpoint --channel --session-ref [--task …] [--next …] [--unresolved …]* [--decision <id>]* [--assignment <id>]* [--cwd <path>]`
  Reads the worktree: HEAD sha, branch, base (merge-base with `main` when
  resolvable), `dirty`. Preserves: pushes HEAD to the seat/owner wip ref
  (`refs/heads/wip/<role-or-'owner'>/<session8>`) with the caller's own key via the
  existing git credential path. Working-tree capture is **complete or
  enumerated**: with a temporary index (`GIT_INDEX_FILE`), `git add -A` then
  `git diff --cached --binary --full-index <headSha>` so staged, unstaged,
  untracked and binary content all land in one patch bound to `headSha`;
  files that exceed the per-file bound (256 KiB) or the patch bound (1 MiB total,
  published as a NIP-34 patch event or, above the relay event limit, as a
  Blossom blob referenced by hash) are **omitted by path and each listed under
  `missing`**. The checkpoint carries `revision.preserved: "all" | "partial" | "none"`;
  `dirty: true` with a patch never implies all bytes preserved unless
  `preserved == "all"`. A failed push is recorded under `missing` with the
  reason. Publishes the 44247 `checkpoint`. Prints the event id and every
  artifact/missing line. Never rewrites history, never force-pushes. Test
  staged-only, untracked, binary and tracked edits together.
- `handover claim --channel --session-ref --body <providerAuthorityPubkey>|--body-self`
  Publishes `takeover` at the current head; waits for the 40099 receipt
  (bounded); on `StaleHead` re-reads the chain once and reports the accepted
  claimant instead of retrying blindly. Exit 5 on a lost race.
- `handover continue --channel --session-ref [--cwd <checkout>] [--body …] [--reconstruct|--native]`
  1. Folds authority + handover; refuses if retired, if the caller lacks standing,
     or if no authorized checkpoint exists (unless `--allow-no-checkpoint`, which
     reconstructs from the transcript tail and labels it so).
  2. Reachability: the claimant's candidate original execution is "reachable"
     iff the coordination fold shows a live 24223 lease for its current
     generation. `--native` requires it; `--reconstruct` skips it; default picks
     native when reachable and the caller already holds a grant on the umbrella
     (the existing grant is the resource consent this increment reuses — stated
     limit), else reconstruct.
  3. Claims (as above) with `body` = P_A for native, the caller's own provider
     for reconstruct.
  4. Native: sends the checkpoint's `nextAction` as a `thread.turn.start` to the
     original target (`bee sessions send` path) and publishes `continuation
     {mode: native-resume}` once `turn_started` is observed (or the refusal it got).
  5. Reconstruct: in `--cwd`, fetches the wip ref and checks it out on branch
     `handover/<session8>` at the checkpoint's `headSha` (refuses if the relay's
     30618 does not show that sha), applies the patch artifact with
     `git apply --check` first, then `git apply --binary --3way --index` against
     that exact base; a failed or partial application is reported per path
     under `missing` and never leaves a half-applied tree; records
     `recovered`/`missing`; publishes `session.create` with
     `sessionRef`/`genesisRef`/`repoRef`, the caller's provider, and
     `initialTurn` = rendered checkpoint (task, decisions, revision, tests,
     unresolved, next action, artifact list, missing list); waits for the
     `created` receipt; publishes `continuation {mode: reconstructed, target}`.
- `handover status --channel --session-ref [--json]` prints the fold: claim,
  claimant, body, since, latest checkpoint (author, revision, artifacts,
  missing), continuations, retirement, and each execution's fence state.

Every step prints what it verified and what it did not.

## 5. Desktop (Lane U)

- Decoders: 44247 (`codingSessionHandoverWire.ts`), `takeover`/`transfer` +
  `bodyPubkey` in the authority projection and 40099 receipt reader, 44223
  `handover` field.
- Model `codingSessionHandoverModel.ts`: from the coordination fold + authority
  + handover fold → `{ claim, activeBody, continuation, latestCheckpoint,
  viewerMayContinue, viewerIsClaimant, thisExecutionFenced, retired, evidenceLinks }`.
- Surface in the session workspace (`CodingSessionParticipantBar`/header area,
  new `CodingSessionHandoverPanel.tsx`): a status line — "Active: {claimant}
  on {body} · reconstructed from checkpoint {link} · recovered … · missing …" or
  "Native continuation by {claimant}"; on a fenced execution: "Fenced: {claimant}
  took over {age}. This execution cannot act. [Take back]"; and a **Continue
  work** action when `viewerMayContinue` (founder or live operator, not the
  claimant, and the current body is not `provider_reachable`). The action runs
  the claim + reconstruction through the existing create path
  (`useNewCodingSessionCreate` with `sessionRef`/`genesisRef`, initial turn from
  the checkpoint) after a narrow Tauri command `handover_prepare_checkout`
  (`desktop/src-tauri/src/commands/handover.rs`: fetch wip ref, checkout at sha,
  apply patch; reports recovered/missing) — no technical setup wall: the workdir
  picker is the existing one. Retired umbrellas render "Deleted on {date}" and
  offer nothing.
- Mock bridge: 44247 serving, takeover receipts, a fenced metadata fixture.
- Tests: JSDOM unit tests for the model and panel; Playwright: two owners
  (viewer as B with a grant; A's execution unverified) → Continue work →
  reconstructed row with evidence links; fenced view as A; retired view; narrow
  width and 250% zoom; hash-distinct screenshots on port 4176.

## 6. Composition (Lane A) — `scripts/handover-acceptance.sh`, `just test-handover`

Real relay (scratch Postgres, Redis DB 14), real `bee`, **two concurrent
providers** with distinct keys and state dirs (owners A and B), fake bash ACP
adapter (the `ci-continuation-acceptance.sh` pattern), two git checkouts of one
relay-hosted repo. Steps, each a PASS line:

1. A creates umbrella + grants B; A's seat commits and the wip ref lands (30618).
2. A checkpoints (44247) with the wip artifact; uncommitted change → patch.
3. `kill -9` P_A. B `handover continue` → claim accepted, reconstruct chosen
   (no live lease), checkout at headSha, patch applied, `session.create` on
   P_B, `created` receipt, continuation published with `recovered` lines.
4. Restart P_A: `recover` re-derives the fence; A's `bee sessions send` to E_A
   → `HANDOVER_FENCED` receipt; A's replayed pre-crash turn → fenced too; P_A
   publishes E_A `disconnected` + `handover`.
5. Racing claims: A and B publish `takeover` at the same head; exactly one
   40099 receipt; the loser's CLI exits 5 naming the winner.
6. Revoke B → fold shows no claim; regrant B → still no claim; B must claim again.
7. Replay: re-deliver the accepted takeover receipt and the continuation event
   twice → one fence, one continuation, no new execution.
8. Missing artifact: checkpoint whose wip sha is absent from 30618 → `continue`
   refuses reconstruction of that sha, discloses, proceeds only with `--allow-no-artifact`.
9. Interrupted publication: kill B's CLI between claim receipt and continuation
   → `handover status` shows claim without continuation; rerun `continue` is
   idempotent (same claim, one execution).
10. Retirement: A deletes the session (`bee sessions delete`); P_B restart →
    record retired, no metadata, `SESSION_RETIRED` on send; live deletion
    observed by a running P_A retires too.

Native-continuation leg (P_A alive): B `continue --native` → `turn_started` on
E_A, continuation `native-resume`. Native Windows acceptance: **deferred,
labelled**. Mock UI is never cross-machine proof.

## 7. Lanes and ownership (nobody commits)

- **K — protocol core (first).** `crates/buzz-core/src/{kind.rs (44247 const), coding_session_authority_transition.rs, coding_session_authority_claim.rs (new), coding_session_handover.rs (new), coding_session_handover_fold.rs (new), coding_session_payload.rs (two consts), lib.rs (mods)}` + tests + fixture writer; `crates/buzz-db/src/event.rs` (acceptance rules, refusal variants); `crates/buzz-relay/src/handlers/{ingest.rs (44247 envelope + scope/membership arms; refusal messages), side_effects.rs (receipt bodyPubkey)}` + tests; `crates/buzz-session-provider/src/authority.rs` (claim in `fold_current_authority`); `crates/buzz-sdk` builder for 44247 and takeover; `desktop/src/shared/constants/kinds.ts` (44247 const + registry count).
- **X — provider/host.** `crates/buzz-session-provider/src/{state.rs, commands.rs, lib.rs (recover, subscribe kind 5, claim consumption, retirement), native_restore.rs, seat_requests.rs, team_wake.rs (admission), publish.rs/payload.rs (metadata handover field), new retirement.rs}` + tests under `src/tests/handover_*`; `desktop/src-tauri/src/managed_agents/actor_seats_restage.rs` (skip retired/fenced) + tests.
- **C — CLI.** `crates/buzz-cli/src/lib.rs` (variants), `crates/buzz-cli/src/commands/sessions/handover*.rs` (new), `client.rs` (narrow calls if needed), `patches.rs` builder reuse (no edit unless a `pub fn` split is needed — name it).
- **U — desktop.** `desktop/src/features/coding-sessions/lib/{codingSessionHandoverWire.ts, codingSessionHandoverFold.ts, codingSessionHandoverModel.ts, codingSessionMissionAuthority.ts (takeover/transfer/bodyPubkey), codingSessionMetadata decoder (handover field)}`, `ui/CodingSessionHandoverPanel.tsx` + mount in `CodingSessionWorkspace.tsx`/`CodingSessionParticipantBar.tsx`, `desktop/src-tauri/src/commands/handover.rs` (+ mod/handlers lines), `desktop/src/testing/e2eBridge*.ts` (44247/receipt fixtures), `desktop/tests/e2e/coding-session-handover.spec.ts` + helper + `playwright.config.ts`.
- **A — composition.** `scripts/handover-acceptance.sh`, `Justfile` recipe, `docs/COLLABORATION_TWO_MACHINE_ACCEPTANCE.md` §6 (handover runbook).
- **R — independent adversarial review** (reads only).

Sequence: K first (shared types, fixture); X, C, U in parallel against K's
types; A after C and X; R after all. Finalizer commits with signoff.

## 8. Gates

`cargo test -p buzz-core -p buzz-db -p buzz-relay -p buzz-session-provider -p buzz-cli -j 3`
(relay/db integration needs scratch Postgres + Redis 14 via the existing test
helpers), clippy `--all-targets -D warnings`, `cargo fmt --check`, Tauri crate
tests + clippy, `pnpm check` + `tsc` + full desktop unit suite, Playwright on
4176, `just test-handover`. Own Cargo target dirs
(`/Users/brian/Projects/beekeeper/cargo-target-fable-handover{,-tauri}`), never
`/tmp/astra-workspace-admission-target`.

## 9. Stated limits

- Reconstruction is a new execution: native context is not migrated (the
  cursor never leaves the original disk); the label says so.
- Resource consent = the existing operator grant on the umbrella; a per-body
  consent field on 44245 policy is named as the follow-up, not implemented.
- Retirement verifies relay deletion receipts (any authorized deleter) and
  founder-signed deletions with confirmed absence; project-owner deletions that
  predate the receipt are not provider-verifiable and are a stated limit.
- v1 hands over the whole session (umbrella scope); sibling executions are
  fenced with it and the surfaces say so.
- Version skew: consumers built before `takeover` fail closed on the chain
  (existing behaviour); this ships as one release.
- Uncommitted bytes larger than the patch bound, or on a machine that never
  checkpointed, are disclosed as missing, never recovered.
- Composition uses a stub adapter and two local providers; native Windows and
  real-model acceptance are deferred and labelled.
