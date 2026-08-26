# Sessions — living state

**The one document to read first.** Everything else is either a binding
authority (§4), a protocol for a specific experiment, or history. This file
is updated on every build and whenever live use produces a finding; if it
disagrees with an older document about *current state*, this one wins.

> **2026-08-26 — the relay is canonical; GitHub is a bridge-fed follower.**
> Verification first: nothing server-side ever pushed to GitHub — Woodpecker's
> forge *is* GitHub (`WOODPECKER_GITHUB=true`), so GitHub pushes trigger CI,
> and the only thing keeping GitHub current was the per-clone dual push URL.
> That is now inverted for beekeeper: `hive-mirror-bridge.service` on the
> forge (`crates/buzz-mirror-bridge`) subscribes to the relay's kind:30618
> ref-state events and syncs relay → `/srv/git/beekeeper.git` → GitHub within
> seconds; the hourly `git-mirror.timer` is the reconcile fallback. Deployed
> by `scripts/forge/setup-hive-mirror.sh`; direction is per-repo
> `mirror.fetchRemote`/`mirror.pushRemote` config (other `/srv/git` repos
> still pull GitHub). Forge identities: deploy key
> `forge-mirror-beekeeper-rw` (write), Nostr pubkey `f08a4e42…0627` — relay
> member *and* #general member, because the git read gate 404s repos to
> non-channel-members. Forge git upgraded to 2.55 (credential `authtype`
> needs ≥2.46). Push **only to `origin`** now; the dual push URL is retired
> (`git config --unset-all remote.origin.pushurl` on clones that still carry
> it) — a direct GitHub push now merely races the bridge, which was observed
> beating a dual-push's GitHub leg with its own commit on day one. The
> divergence that motivated all this happened the same day: a GitHub-only
> push and a hive push split `main` for an hour.
> `docs/INTEGRATION.md` § Remotes and § The relay is canonical carry the
> durable version of this.

> **2026-08-21 — the branch ceremony is gone, and so is the repo it ran in.**
> This repo is now `agiterra/beekeeper`: a single `main` branch, ordinary topic
> branches, upstream **merged** in occasionally. **Since 2026-08-23 topic
> branches are rebased onto `main`, not merged into it** — upstream stays a
> merge, and `docs/INTEGRATION.md` § Landing a topic branch explains why the
> two differ. `scripts/integrate.sh`,
> `CONTRIBUTING-FORK.md` and the split-map bookkeeping are deleted;
> `docs/INTEGRATION.md` now describes only what is still live (CI, caching,
> autodeploy). The near-pristine upstream mirror plus the single
> ci.agiterra.org patch moved to `agiterra/buzz`, which will serve a **vanilla**
> relay at `lightyear.agiterra.org`; Bee Keeper gets a new relay at
> `hive.agiterra.org`. **All data on the current relay is being abandoned**, so
> every deployment fact below this banner describes a relay that is going away.
>
> **Host side, as of 2026-08-22.** The forge mirror and Woodpecker are done:
> `/srv/git/beekeeper.git` is a true mirror (`+refs/*:refs/*`, read-only deploy
> key `forge mirror (agincus)`) and joined `git-mirror.service` automatically
> because the updater globs `/srv/git/*.git`; `agiterra/beekeeper` is Woodpecker
> **repo id 2**, active, branch `main`, Trusted→Volumes on (the gate's
> `/srv/ci-cache` mounts require it, and new repos default it off). Still owed:
> `buzz-autodeploy` remains hardcoded to `agiterra/buzz` / `integrated` /
> `/opt/buzz` / the `buzz` instance, so **hive has no automated deploy** and
> runs a hand-built image. GitHub Actions are **disabled** on
> `agiterra/beekeeper` — eleven of the eighteen workflows lack the
> `github.repository == 'block/buzz'` guard and failed on every push. The
> workflow files stay in the tree on purpose: deleting them would conflict
> against upstream on every future merge.

_Last updated: 2026-08-26 — the mirror inversion landed and is live (banner
above); `main` is identical on the relay and GitHub, fed by the bridge._

_Previously: 2026-08-25 — a live-driven day. §2 items 45-50 (the picker
rebuild, the session ceiling and silent-turn budget as settings, and the
redaction rework) are fixed and pushed through CI #24; items 51-53 and 55-57
are the full-screen session pass (landed on `main` as `b2298102` on
2026-08-26 after a rebase onto Andy's shelf fix), item 54 records the Claude
background-shell / unresolved-prompt failure, and item 59 (renumbered from a
colliding 51) is Andy's fifteen-minute hang. The 2026-08-26 mirror-inversion
commit rewrote this block from an older copy and dropped items 52-impl
through 57; restored the same day from `b2298102`._

_Previously: 2026-08-22 — Phase 4 is complete. The rebrand landed on `main`
(`d90c24d14`), beekeeper has a real gate (Woodpecker repo 2, first pipeline
green at `62bcaa223`), hive serves Bee Keeper, and **lightyear has been rebuilt
as a vanilla relay** with a wiped database and a rotated keypair. What remains
is automation: neither relay has a deployer._

_Previously: 2026-08-21, preparing the lease-backed **Agent Progress**
surface for the next build (§2 item 36). `build/2026-08-21.1` is no longer a
candidate: Woodpecker #109 passed at `04a087e4a`, and the first live digest
proved the deployed lease end to end — one provider-reachable session whose
last durable observation was 9,119 seconds old. The same read was honestly
partial because several session channels were inaccessible._

---

## 1. What is live

| | |
| --- | --- |
| Deployed (Bee Keeper) | `beekeeper-relay:7d224a8b6` on **hive.agiterra.org**, observed running there 2026-08-23 (deploy time not recorded), with `a961fb277` pushed to `main` the same morning and awaiting the deployer's next tick. (Was `0be70d424`, shipped 2026-08-22 12:24 UTC by its own deployer (`deploy/autodeploy/`, installed as `beekeeper-autodeploy.timer`) — the first automated Bee Keeper deploy. Its community row was created by `ensure_configured_community` on first boot from `RELAY_URL`; owner `6cbdf445…92b68df2`, key imported into the desktop keychain and the server copy deleted. Relay identity (NIP-11 `self`) is **`1fb029d0…c09ab336`** — recorded here as the baseline, because `BUZZ_RELAY_PRIVATE_KEY` auto-generates when unset, and a relay that silently rotates its key on every restart evicts every client cache. Both it and `BUZZ_GIT_HOOK_HMAC_SECRET` are persisted at 64 chars; check `self` against this value after any deploy.) |
| Deployed (vanilla) | `buzz-relay:12201c49b` on **lightyear.agiterra.org**, rebuilt from scratch 2026-08-22. Database dropped and recreated — **32 migrations applied, max version 32**, which is vanilla's schema and not the fork's 40, so the tree is provably upstream. Relay keypair rotated: NIP-11 `self` moved `2ba5c5e7…d13dda7` → `f85e9e21…7b08455c`, matching the key generated in-container. Fresh owner `180d54c0…d32805c8`, bootstrapped by the relay itself. NIP-11 reads `Buzz Relay` / `github.com/block/buzz`. Owner key backed up and `/opt/buzz/.owner-key` deleted. Autodeploy tracks it, pinned to `repo_id = 1`. |
| Superseded by the above | `build/2026-08-21.1` (`04a087e4a`) on lightyear — Woodpecker #109 passed all jobs; a live cold digest then observed a current kind-24223 lease and classified a session last durably observed 9,119 seconds earlier as provider-reachable, proving the new relay rather than the pre-lease build answered. |
| Assembly | Agent Progress candidate on top of `build/2026-08-21.1`: one shared coordination fold now supplies Pulse and the global `/agent-progress` preview surface; item 36 records the exact product and gate claims. It is not shipped until its new build tag and Woodpecker result exist. |
| Unshipped locally | the `just dev` nokeyring fix, three verified-missing session-stability fixes (§3), the project-roster git ACL (landed on `main`, not deployed — see §2), and the **coding-session responsiveness fixes** (2026-08-24, landed on `main`, not deployed, **not yet exercised against a live relay**). Two independent multi-second stalls. (1) *A sent turn took 2–4+ seconds to appear.* The provider drained its durable outbox one row per `OUTBOX_TICK` (2s) via `flush_one`, and nothing else in the runtime loop flushed it — the relay-event and session-event arms only handled leases — so the `user_prompt` 44225 echoing the person's own message queued behind the turn's `running` metadata (`Priority::High` drains first) and waited two ticks on a timer whose real job is hot-reloading a config file; the same throttle capped a streaming turn at 0.5 events/sec. The loop now waits on `Outbox::next_retry_delay()`: empty parks the arm, a failed row returns its backoff, a burst drains at relay speed. Still one row per pass, and the arm sits last in the biased select. `OUTBOX_TICK` → `RUNTIME_TICK`. Client-side, the composer clears its editor and records the turn *before* awaiting the publish, and `codingSessionPendingTurns.ts` renders it as its own clearly-local row — never merged into the trusted transcript — retired by the provider's verified echo (one echo per row) and restored to the editor on a failed publish. (2) *Switching sessions showed ~1s of "Loading".* Each session lives in its own transport channel, so a switch changed the ingress scope and discarded a fully verified `TrustedCodingSessionIngressStore`, paying for a live subscribe *then* a history fetch before the routed generation could resolve. Display scopes now keep that store warm (`codingSessionIngressStoreCache.ts`, LRU of 6, keyed by authority identity **and** channel set so facts never cross an allowlist, reset in `resetCommunityState()`); command scopes are excluded. Two honesty bugs fell out and are fixed: the pre-effect render returned an empty snapshot with `isLoading: false`, which `resolveCodingSessionWorkspace` renders as "this exact generation is not available in the relay catalog" for a session just clicked; and `historyLoading` only became true at `onAttemptStart`, publishing "settled and empty" across the window between arming a scope and its first request. Also landed: the project shelf reads `Working`/`Idle` rather than `Reported working`, with the metadata-is-not-liveness caveat moved to the row's hover (`projectSessionObservationTitle`). |
| Latest assembly | `build/2026-08-21.1` — the complete verified-liveness stack, the CI-105 state-lock recovery (item 34), and the fenced-session briefing (item 35). Woodpecker #109 passed at `04a087e4a`; the live 9,119-second-old observation above is the first meaningful deployed lease acceptance. Agent Progress remains the next candidate until its ceremony completes. |
| Built, awaiting acceptance | **Project Pulse Slice 1** — five signed commits on `wip/project-pulse` (`aced60f2`…`d235189a`, 2026-08-19): kind 44240 end to end (core contract, relay ACL on every read surface, `bee pulse` CLI, ACP digest injection, Desktop screen behind the `project-pulse` preview flag). Gated green (live e2e 11/11, desktop 5832/5832, conformance 42/42, clippy/fmt clean). Blocked on Brian's §5.8 manual acceptance (`docs/PULSE_SLICE1_ACCEPTANCE_RUNBOOK.md`); split ceremony pre-computed in `docs/PULSE_SLICE1_SPLIT_MAP.md` (since deleted
along with the ceremony — recover from git history if ever needed). Plan: `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md`. Next build queued: `docs/REHYDRATION_HARDENING_IMPLEMENTATION_PLAN_2026-08-19.md` (verified; zero file overlap with Pulse). **Shipped 2026-08-19 night as `build/2026-08-19.3`** — split onto `feature/project-pulse` + `integration/glue` and pushed to both remotes. That build shipped **without** the UX-fix pass, which was still uncommitted in `/Users/brian/Projects/buzz-uxfix` when the window closed. **The UX pass then shipped the same night as `build/2026-08-19.4`** — all 15 critique findings plus the error-card fix, folded as per-file diffs onto `feature/project-pulse` (`ebf5085c`, `877723fc`) and `integration/glue` (`db8614cc`) per the split map's EXECUTED banner, changed-line multisets verified identical (2,689 pulse-owned + 20 glue-owned lines) and `git diff wip/pulse-ux-fixes integrated-build` clean of every product hunk. Gate cited: desktop 5845/5845, fold conformance 42/42, `tsc --noEmit`, px-text guard; the 62 e2e-smoke failures were reproduced at `1ac2ac51` in a throwaway worktree and are therefore inherited, not caused by this delta — CI re-gates on push. §5.8 manual acceptance is **still owed**, and those 62 inherited smoke failures are still unexplained (§3) |

Shipped in this arc: durable names (R26), closure/stop separation (R27),
verified rehydration + systemPrompt-first bootstrap, continuity disclosure
end to end, reconnect that follows its generation, visible turn refusals,
foreign-member authority resolution, project-claim inheritance on join,
operator attribution.

Proven live, not merely tested: the full authority ladder (observe → refuse →
grant → grantee turn → stop refused, 2026-08-18 local); cross-provider
rehydration on the deployed relay; operator attribution rendering correctly
across two machines simultaneously.

## 2. Open — ordered, each with its evidence

1. **A duplicated create silently kills rehydration.** Observed end to end in
   Session_Test_8_18_8PM: one click produced two Codex executions 8 ms apart
   (`ef518a28`, `7c4ed948`, one command id). The projector then refused —
   "context fact conflict: command … has more than one" — so both executions
   started `Fresh` with no context tooling, and the joined agent correctly
   told the operator it could not see the prior work. The refusal is right;
   the duplicate is the bug. Its cause (one provider instance per state
   directory) shipped in this build — **re-test before assuming it is
   closed.** ~~The disclosure remains useless: the operator sees
   `session_fresh` while the reason lives only in the provider log. Surface
   the bail-out reason.~~ **Disclosure half fixed 2026-08-20 by `02824ea5`
   + `6f85b431`** — the bail-out reason is now an enumerated slug carried on
   the signed status item (`CONTEXT_UNAVAILABLE_REASONS`, eleven of them,
   `crates/buzz-core/src/coding_session_payload.rs:595`; a slug outside the
   set is dropped and the key omitted rather than sent as `null`, because
   "no reason observed" is a different fact from "this item carries no
   reason"). The desktop transcript renders it as a clause — "Restarted
   without prior context — <clause>" — and falls back to the generic row for
   an unrecognised slug rather than leaking a wire token into the UI.
   **The duplicate itself is NOT fixed by that work**: item 1's cause (one
   provider instance per state directory) still owes the live re-test above.
   What changed is that when it does happen, the operator is told why.

2. **"What was attempted" is unstructured for some adapters.** A turn records
   the downstream richly — full tool output, `exit_code`, `isError`, duration,
   tokens — but the upstream is adapter-dependent: `claude-agent-acp`
   populates `tool.input` verbatim; `codex-acp` leaves it `{}` and puts the
   whole command in `tool.toolName` as prose. Nothing supports "has this exact
   thing already failed", which is why failed attempts repeat. `toolName` also
   carries host paths into signed events.

3. **Rehydration is create-only.** ~~A reattached execution gets no context
   MCP.~~ **Fixed in code 2026-08-18 by `b9de9a6d`** — verified 2026-08-19:
   `session.rs:646/:667` now pass `mcp_servers.clone()` into
   resume/load, and `lib.rs:1081-1098` builds a real `rehydration_mcp` on
   `resume_session`. The 2026-08-18 live observations (resumed Codex lost
   its package; Claude gen 2 "did not see that tool available") predate the
   fix. Still owed: a live re-test of resume-with-context, and confirmation
   the fix is in the deployed build (it postdates `build/2026-08-18.7`).
4. **The package is a start-time snapshot.** ~~Provenance says
   `complete: true` meaning "complete when projected", which reads as
   "current". A joined execution never learns what a sibling did afterward
   (observed: Codex reported 16/16 complete while Claude had advanced to 6
   turns). Fix: time-bound the provenance, then add refresh.~~ **Fixed
   2026-08-20 by `2df636ef` + `bcadafe2`** — both halves of the stated fix
   landed. *Time-bound:* every response carries `projectedAtMs` and a derived
   snapshot age (`crates/buzz-dev-mcp/src/session_context.rs:672`, `:685`),
   and the bootstrap prefix instructs the agent about that age
   (`session.rs:1999`). *Refresh:* a starting turn — not an interrupt —
   spawns a bounded re-projection
   (`crates/buzz-session-provider/src/lib.rs:1411`, `spawn_context_refresh`
   at `:1950`), floored at `CONTEXT_REFRESH_MIN_INTERVAL_MS` = 60 s (`:106`)
   and started rather than awaited so the turn never waits on a relay fetch.
   Each refresh is a new write-once generation, `create_new(true)` at its
   final path with no `rename(2)` step (`context_store.rs:74-120`) —
   `rename(2)` replaces its destination and would repeal the write-once
   guarantee the reader relies on. Only generation 0 may create the package
   directory, so a refresh that loses the race with `stop_session` fails
   `NotFound` instead of resurrecting verified private context past an
   operator's stop. The sidecar serves the newest generation it can fully
   validate and names that generation in every envelope. **Still owed: a live
   two-execution re-test** that a sibling's later work now actually arrives.
5. **`session_history` page cap is 20** ~~(`session_context.rs:23-24`).
   Reading 101 items took six calls plus a hard `limit must be between 1 and
   20` error. Untenable as sessions grow.~~ **Fixed 2026-08-20 by
   `bcadafe2`** — `limit` now defaults to 200 and caps at the package ceiling
   itself, imported rather than restated so the page cap and package cap
   cannot drift apart again (`MAX_HISTORY_LIMIT = MAX_CONTEXT_HISTORY_ITEMS`
   = 4096, `session_context.rs:42`, `coding_session_context.rs:32`). A page
   also ends at a shared 128 KiB response budget (`MAX_HISTORY_PAGE_BYTES`,
   `:50`), whichever bound comes first, and `stoppedBy` names which one did
   (`limit`, `pageBytes`, `end`, `cursorMiss`). Paging is now cursor-first —
   `since` = the eventId of the last item read — because offsets cannot
   survive a package refresh and cursors can; a `since` the served package no
   longer carries returns no items with `cursorResolution=not_in_package`
   rather than silently restarting at zero. `view="index"` returns metadata
   only (eventId, a 64-byte preview, a targets-legend index) at roughly 380
   items per call. The 128 KiB budget was deliberately **not** tuned upward
   to make "one call" true, because that trades a hard error for a blown
   context window.
6. **Provenance has an unexplained delta.** ~~`sourceEventCount` counts the
   proof graph (`1 + authority_links×2 + names + goals + per generation
   (3 + transcript)`, `context_projector.rs:945-955`), not content, so it
   exceeds `totalHistoryItems`. Nothing is missing, but the package never
   says so and a careful agent had to flag it as unexplained.~~ **Fixed
   2026-08-20 by `02824ea5` + `2df636ef`** — the package now ships the
   arithmetic as a structured note instead of leaving a careful agent to do
   it: "sourceEventCount N includes M non-content proof events (… genesis, …
   authority, … name, … goal, … per-generation bookkeeping) in addition to …
   transcript events; … became history items."
   (`context_projector.rs:1048-1064`). It is backed by the structured
   `sourceEventBreakdown` field, is emitted even when the note budget is
   full — the reconciliation "never fails soft" (`:2226-2243`) — and is
   omitted only when the numbers already reconcile (`:2169-2177`). The
   first-turn brief now also prints `sourceEventCount` inside its `snapshot`
   object beside `sourceEventBreakdown`, because that brief ships standalone
   as the ACP bootstrap prompt, where the agent was previously handed six
   breakdown terms and a rule saying they reconcile a field the brief never
   showed it.
7. **An execution whose provider identity is gone is a dead row, silently.**
   The provider's `SessionRecord` and opaque resume cursor live in one
   app-instance state directory keyed by provider pubkey. Lose that identity
   — reinstall, new machine, keychain loss, or in dev a branch-derived slug
   change (`scripts/instance-env.sh`) — and no provider claims those
   executions: Reconnect goes unanswered (`Ignored::UnknownTarget` publishes
   no receipt, by design) and Stop is silently ignored, so the row is stuck
   `disconnected` forever and the session is unendable (R27's tail).
   Observed 2026-08-18 by switching the assembly worktree's branch.
   The durable session is unharmed — transcript, goal, roster and history
   all survive, and Add provider still rehydrates a new execution from
   them — but the UI never says so. It has what it needs to: compare the
   create's `providerAuthorityPubkey` with the local provider's pubkey and
   say "this execution belongs to a provider this computer no longer has;
   start a new one and its history comes with it." Wants a founder-signed
   way to retire an orphaned execution, too.

8. **Desktop grant-awareness (A8-shaped).** Granted operators still see a
   gated composer; every new session needs its members added by CLI, because
   each create stamps a fresh transport channel. The observation half needs
   no new protocol and is not blocked by A6.
9. **Stale clients are invisible.** An old client shows "ungoverned — adopt
   to govern", falls open, and silently swallows refusals. There is no
   version signal in the UI.
10. **P1 has never run.** Harness (`scripts/p1-seed-spike/`), judging protocol
   (`P1_JUDGE_SCRIPT.md`), and matrix (`P1_CONTINUITY_MATRIX.md`) are ready.
   Gates the whole seed/checkpoint track.
11. Smaller: stale-Reconnect echo port (`a92fd728` absent from this line);
   `rejectedAuthorCount` surfaced nowhere; `Loaded` vs `Resumed` conflated at
   the receipt layer.
### Found 2026-08-19 during the Project Pulse Slice 1 build

18. **Rust and Desktop disagree on malformed-44223 admissibility** (found by
   the Pulse fold-parity review). The CLI fold decodes with strict
   `decode_coding_session_metadata` (exact key set); Desktop uses the
   permissive `parseBuzzCodingSessionMetadata`, so a 44223 carrying an
   unknown key is dropped by one and kept by the other. Documented in
   `pulseFold.ts`'s module doc with an instruction not to bank a conformance
   vector on it. Needs a product call: pick one decoder as the contract.
19. **The ACP-injected Pulse digest carries entries only, no session facts**
   (deliberate Slice 1 deviation, disclosed in the injection via
   `SESSIONS_OMITTED_LINE`, `pulse_fetch.rs:66`). An overlap visible only in
   session facts will not reach the agent until the session fold is built
   into `buzz-acp`. Not lying, but not done.

### Found 2026-08-20 in the first live use of Project Pulse

Items 21–23 are fixed in `build/2026-08-20.2`. Item 20 is **open** and needs
Brian's product call.

20. **A live session ages out of Active work.** `publish_metadata` fires only
   on transitions (`crates/buzz-session-provider/src/lib.rs:905` create,
   `:1170` idle, `:1194` stopped, `:440` disconnected); there is no
   heartbeat, so an idle-but-alive session emits nothing and after
   `PULSE_ACTIVE_WINDOW_SECONDS` (1800,
   `desktop/src/features/project-pulse/lib/pulseFold.ts:67`) it falls to Last
   seen. Observed live 2026-08-20: PulseTestV2 was alive while Pulse showed
   "Idle · last observed 30m ago" and Active work was empty. Widening the
   window re-admits the ghosts the gate was built to stop; the fix is a
   provider heartbeat, so silence honestly means absence. Needs a product
   call on cadence vs permanent event volume. *Re-verified at the
   2026-08-20.2 ceremony, and the mechanism is stronger than "only on
   transitions":* `publish_metadata` itself (`lib.rs:1786`) returns early
   when the serialized content equals the last publication (`:1794-1799`), so
   even the periodic git-probe republish (`:2068`) emits nothing when nothing
   changed. The silence is structural, not incidental. **Closed for Project Pulse
   2026-08-20 night by item 28** (the kind-24223 provider lease renews on
   actor liveness, not on turn activity). **Still open for the coding-session
   surfaces** — the session card, project shelf and sidebar do not read the
   lease, so they can still print "Idle · last observed 30m ago" over a live
   session.
21. **Entries were buried below the session groups**, so the screen led with
   what Pulse *observed* rather than what people *claimed*. Fixed: entries +
   superseded now render above the session groups, with a counts line and an
   entries empty-state carrying the same three-way honesty split the sessions
   empty-state already had.
22. **Repeated executions of one session rendered as repeated full cards.**
   Fixed **at presentation level only**: `groupPulseSessionExecutions`
   (`desktop/src/features/project-pulse/lib/pulseFormat.ts`) keys on
   `session.sessionRef ?? session.targetKey` and collapses them into one card
   with a `> N executions` disclosure. The digest shape, `pulseFold.ts`
   identity and the 42 conformance vectors are untouched and re-verified
   green — what the fold calls one session did not change, only how many
   cards that draws.
23. **Card noise, including a redundant Closed/Ended pair.** Fixed: the
   commit-confirmation qualifier moved inline into the chip row (~19
   dedicated lines gone), the "observed Nm ago" chip renders only on active
   sessions (non-active headers carry "… · last observed Nh ago"), and the
   "Closed" chip is suppressed only when the status label already reads
   "Ended". The honest tri-state survives; only the duplication went.

The fix for 21 introduced, and the verify pass caught, one honesty defect
before it shipped: the new counts line asserted an *absence* over a read that
admits it lost data — a partial read printed a bare "no entries no sessions",
a claim about the project made from a read that returned no claims.
`ProjectPulseView.tsx` now derives `countsAreLowerBounds` and states floors;
at zero the floor reads "no entries in what this read returned", a fact about
the read rather than about the project.

### Found 2026-08-20, second live-use pass

Items 24 and 25 are fixed in `build/2026-08-20.3`. Items 26 and 27 are
**open** and need Brian's product call.

24. **Signed transcripts were never redacted, and a title could eat ACP's
   `kind`.** NIP-CST :43 says the published `item` is "deeply redacted" and
   that host paths and raw provider objects are forbidden. The sanitizer
   existed and worked —
   `buzz_core::coding_session_context::sanitize_coding_session_context_content`
   — but it ran only when building the *private* rehydration package, so the
   one item stream that leaves the machine was the one stream nobody
   redacted. Reproduced against the unfixed translator: `{"kind":"tool_call",
   "tool":{"input":{"file_path":"/Users/brian/Projects/buzz/secret.rs"},
   "toolName":"Read /Users/brian/Projects/buzz/secret.rs"}}` reached
   `sign_with_keys` verbatim. The second half was `Null` where a
   discriminant belonged: `tool_name()` fell back `toolName → title → kind`
   (`crates/buzz-session-provider/src/transcript.rs:382-391` before the fix),
   folding ACP's *discriminant* into the *name*, so any adapter that also
   wrote a title silently erased its own kind. **Fixed:** `fit_item`
   (`transcript.rs:289-295`) is now the redaction seam — it calls the shared
   sanitizer and then delegates the old body to a private `shrink_item`
   (`:297`); redaction runs *first*, because eliding a path makes the value
   longer and it is the fitted size that has to respect the 32 KiB cap.
   `fit_item` has exactly one non-test caller,
   `crates/buzz-session-provider/src/lib.rs:1845` in `enqueue_transcript`,
   which is the sole path to `sign_with_keys` (`:1849`) for kind:44225, so
   translator items, payload builders and lifecycle rows are all covered and
   a future producer cannot forget it. The discriminant now publishes as its
   own `toolKind` key on the call (`transcript.rs:226`) and is recalled onto
   the paired result (`:241-255`) from a `tool_kinds` map (`:77`, cleared
   with `tool_names` at turn end, `:170`) — a `tool_call_update` almost
   never repeats it. **ACP marks `kind` OPTIONAL**, so it is written only
   when the adapter actually sent one; an invented discriminant would be a
   worse lie than an absent one, and a third test pins that
   (`transcript.rs:993-995`). Wire-compatible: the tag list is untouched.

25. **Wide content in a coding-session transcript ended in a flat vertical
   cut.** An earlier study blamed a wide `pre` escaping its parent. The fix
   lane **disproved that** in Chromium — `codeOverflowsSectionPx` measured 0
   both before and after, because the `pre` already scrolled. Two real
   findings replaced it.
   *(a) Latent, and the reason it was never spotted.* Each turn renders with
   `content-visibility: auto`
   (`desktop/src/features/coding-sessions/ui/CodingSessionTranscript.tsx:357`,
   utility defined at `desktop/src/shared/styles/globals/utilities.css:8-11`),
   which implies `contain: layout style paint`. Paint containment makes the
   section a **hard clipping box**: an overflowing child is cut with no
   scrollbar and no ellipsis, and `getComputedStyle().contain` still reports
   `"none"`, which is why inspecting the element never explained the cut.
   Proven with a 900px child inside a 400px section. `content-visibility` was
   deliberately **kept** for its perf win; the containment chain below now
   ensures nothing overflows it.
   *(b) User-visible.* Wide code blocks and markdown tables were genuinely
   scrollable, but macOS paints overlay scrollbars that show nothing at rest,
   so a full 48rem column ended in a cut indistinguishable from truncation.
   Scrollbar styling alone was **measured not to work** — `::-webkit-scrollbar`
   pseudos, `scrollbar-width: thin` and `scrollbar-color` all yielded
   `scrollbarLayoutPx: 0`. The fix is `useHorizontalOverflow`
   (`desktop/src/shared/ui/markdown/useHorizontalOverflow.ts:27`), setting
   `data-overflow` from `scrollWidth - clientWidth - scrollLeft` to drive a
   right-edge mask fade that retracts at the end of the scroll.
   Also fixed, a genuine layout bug rather than a perception one: a ~300-char
   unbroken path in a tool-call parameter box ran off the edge with no
   continuation, now `wrap-anywhere`
   (`CodingSessionTranscriptParts.tsx:98,103`). And the composer
   misregistration was worse than estimated, because the padding sat
   **inside** the measure box; gutter and measure are now separate constants
   (`CodingSessionColumn.tsx:20,31`). The 48rem measure is unchanged and
   deliberate — the reference implementation uses the same one. Landed as
   `feature/coding-sessions` `b6032378`, plus one `integration/glue` hunk for
   `StaticCodeBlock`, which exists only above `feature/project-containers`.

26. **OPEN, needs a product decision: `cst-attempt` is deferred.** Item 2
   above wants "has this exact thing already failed" to be answerable; the
   obvious shape is a filterable tag on the transcript event, so a relay
   query answers it without replaying the stream. It did not ship, for three
   reasons that are each independently sufficient:
   - **It is not additive.** Transcript ingress parses an *exact* five-tag
     list — `h`, `cst-v`, `cs-target`, `cst-seq`, `cst-key`
     (`desktop/src/features/coding-sessions/lib/codingSessionTrustedIngress.ts:339-346`,
     via `parseExactTags`) — so a sixth tag requires a CST wire-version bump
     and a migration for already-shipped readers.
   - **The identifier has to be designed, not picked.** It needs domain
     separation by project and an explicit canonicalization version, and the
     *same* value must appear on both the call and the terminal result, or
     "already failed" cannot be joined at all.
   - **It is a disclosure surface.** A hash of a low-entropy command is not
     opaque — `git status` has exactly one preimage — so a dictionary attack
     recovers the command, and a stable per-command identifier visible across
     sessions then becomes a cross-session correlation oracle. The threat
     model has to precede the tag, not follow it.

27. **OPEN: the session-lease / heartbeat work is in flight in another
   lane.** Item 20 above (a live-but-idle session ages out of Active work) is
   untouched by this build and remains open. Its fix — a provider heartbeat,
   so silence honestly means absence — is being built as session-lease work
   in a concurrent lane (`crates/buzz-session-provider/src/lease.rs`,
   uncommitted at ceremony time), and nothing in `build/2026-08-20.3` touches
   it. The cadence-versus-permanent-event-volume call in item 20 is still
   Brian's to make. **Resolved 2026-08-20 night: that lane landed** — see
   item 28. The cadence is 60 seconds against a 180-second relay-clocked TTL,
   and the event volume is zero, because the lease is ephemeral.

### Found and shipped 2026-08-20 night — verified session liveness

This build closes items 20 and 27 for Project Pulse, and only for Project
Pulse. It also records a ceremony collision that destroyed work, because this
repo records findings the day they happen.

28. **Liveness is now evidence, not inference — ephemeral kind 24223.** A
   session was called "working" because its newest stored event was recent;
   a provider that had died an hour ago still read as active work. The
   provider now signs a short-lived lease for one exact execution generation
   and renews it while its actor is alive. Four properties make the claim
   worth trusting, and each was adversarially reviewed before the fold:
   - **Authority-bound.** The relay admits a lease only when its signer
     equals the `providerAuthorityPubkey` of the lifecycle command that
     opened that exact generation
     (`crates/buzz-db/src/coding_session_generation.rs`). Metadata authorship
     establishes nothing.
   - **The clock is the relay's.** TTL comes from Redis `TIME`
     (`crates/buzz-pubsub/src/session_lease.rs`), never from the claimant, so
     a skewed or hostile provider clock cannot extend its own lease.
     `SESSION_LEASE_TTL_SECS` is 180 against a 60-second renewal ticker
     (`crates/buzz-session-provider/src/lease.rs:19`).
   - **Order-independent.** A Lua script fences on `cslease-seq`; a stale
     renewal arriving after a newer one is dropped, and release writes a
     211-second tombstone so a delayed renewal cannot revive a session the
     operator ended.
   - **Never durable.** 24223 lives only in the expiring Redis register, is
     admitted over WebSocket only, and is served back through `/query` from
     that register with the provider's original signature intact. A cold
     history read cannot resurrect an expired lease.

   Pulse therefore reports three distinct states instead of one guess —
   `provider_reachable`, `open_unverified`, `closed` — and
   `PULSE_ACTIVE_WINDOW_SECONDS` is deleted along with the inference it
   encoded. The vocabulary is written down at `CONTEXT.md` (repo root, new).

   **What this closes, stated narrowly.** Item 20's mechanism is gone: the
   renewal ticker is driven by actor liveness (`live_session_ids()`), not by
   turn activity, so an idle-but-alive session no longer falls silent, and
   the cadence-versus-event-volume call item 20 left to Brian is answered by
   construction — 60 seconds, zero permanent events. Item 27's "in another
   lane" is resolved: that lane landed here.

   **What this does not close.** Only Project Pulse reads 24223. On
   `feature/coding-sessions` the lease kind appears in the desktop kind table
   and nowhere else — `git grep -l KIND_CODING_SESSION_LEASE` over
   `desktop/src` hits `pulseQueries.ts`, `e2eBridge.ts` and
   `projectPulse.spec.ts`, all Pulse. The session card, the project shelf and
   the sidebar still infer status from metadata transitions, so the exact
   sentence item 20 was reported against — "Idle · last observed 30m ago"
   over a live session — can still be produced by those surfaces. Item 20 is
   closed **for Pulse** and open for the coding-session surfaces; see §3.

29. **A branch chip offered rows it then refused to show.** Found by an
   independent review of the lease work, not by its own gate, and fixed
   before the ship. `pulseDigestBranches` enumerates a chip for every
   generation's branch, but the row filter matched on the *display*
   generation's branch alone, so a branch only a superseded generation ran on
   got a chip that rendered `0` and, on click, an empty list. The lie was
   symmetrical: the count and the rows agreed with each other and both
   disagreed with the chip the reader had just been offered. The Rust twin
   (`crates/buzz-cli/src/commands/pulse.rs`) was already correct — the two
   implementations of one contract had drifted, and desktop was the outlier.
   Fixed as a single shared predicate, `pulseSessionMatchesBranch`
   (`desktop/src/features/project-pulse/lib/pulseFormat.ts:265`), called by
   both the rows and the counts; the ambiguous helper `pulseSessionBranch` —
   named "the session's branch", meaning "the display generation's branch" —
   is deleted outright. Regression test at
   `ProjectPulseView.test.mjs:879`. The existing corpus could never have
   caught it: every seeded session keeps one branch across its generations.

30. **A ceremony collision destroyed an uncommitted ledger edit. Nothing was
   pushed.** Two ceremony pipelines were pointed at `/Users/brian/Projects/buzz-ship`
   at the same time and interfered; the half-finished assembly was aborted.
   `origin/integrated` and `origin/integration/glue` never moved from
   `8261d1997` (`build/2026-08-20.3`), verified before recovery began, and
   every branch survived. One thing did not: an uncommitted edit to this file
   was lost, and its content is not recoverable. The mechanism was a
   `git checkout <rev> -- .` staging a whole tree over the working copy. Two
   rules follow, both now in §3a: **one agent in a ceremony worktree at a
   time**, and **never stage a tree wholesale** — inspect with `git show` /
   `git cat-file` / `git diff`, and fold with per-file
   `git diff <base> <head> -- <path> | git apply --index`.

   The recovery fold used exactly that, per-file, against
   `LEASE_DELTA_PLAN.md`; 21 of the 23 files folded onto
   `feature/project-pulse` came out byte-identical to the source branch, and
   the two that did not are the module list and the E2E bridge, which carry
   other features' content.

   Also cleared during recovery: today's `rr-cache` entries were quarantined
   to `/tmp/rr-quarantine-2026-08-20` before the rebuild. They came from
   either the collided run or from per-file `git apply --3way` resolutions
   that are branch-scoped by construction — replaying one of those into a
   cross-feature assembly merge would have deleted the shared-terminal kinds.
   The 57 entries banked 2026-08-19 and earlier, which the ceremony actually
   relies on, were left alone.

30a. **A banked rerere resolution was silently corrupting the assembly — for
   the second time.** During the rebuild, `crates/buzz-relay/src/state.rs`
   was auto-staged "using previous resolution" and the result did not
   compile: the recorded postimage interleaved two cache initializers,
   truncating `shell_roster_cache: Arc::new(` and `project_gate_cache:
   Arc::new(` to their opening parenthesis. Rebuilt the file from a clean
   three-way `git merge-file` of the base/ours/theirs blobs — all three
   hunks have an empty base and are pure unions — and then **overwrote the
   `rr-cache` postimage with the verified result**, because leaving the bad
   entry in place is what makes this recur.

   It has recurred before: `97dda378f fix(integration): take the assembly's
   merged relay, cli and tauri files` (2026-08-18) fixed the same four files
   for the same reason, and that commit had itself dropped out of the glue
   series, taking four accurate doc-comment lines in `state.rs` with it.
   They are restored here. Ten more files were auto-staged from the bank this
   run; each was checked by asserting that every line either side added since
   the merge base is present in the result. All ten passed. The lesson for
   §3a: **an auto-staged rerere resolution is an unreviewed merge** — verify
   it, and when it is wrong, repair the cache entry rather than only the
   working tree.

31. **`POST /events` now refuses every ephemeral kind, and says so.** The
   lease work widened the HTTP gate from `KIND_GIFT_WRAP ||
   KIND_PRESENCE_UPDATE` to `KIND_GIFT_WRAP || is_ephemeral(kind)` — the
   whole 20000–29999 range. Verified before folding that no client depended
   on the old behaviour: desktop publishes through
   `relayClientSession.publishEvent` (WebSocket), mobile documents the
   requirement at `mobile/lib/features/channels/compose_bar/helpers.dart:319`
   ("the HTTP `/api/events` endpoint may silently discard them"), the CLI
   routes shell input and agent drafts through `publish_ephemeral_event`,
   `buzz-acp`'s `RelayEventPublisher` is the WebSocket task, and the
   28936 leave request goes over a WebSocket relay client. HTTP previously
   accepted these and dropped them silently; it now rejects them out loud.

32. **A protocol contract changed inside a feature commit: kind 44223.**
   Coding-session metadata is no longer "immutable per generation" but an
   append-only observation history — providers publish on each observed
   transition and consumers fold the newest valid observation per generation.
   No relay-side uniqueness enforcement had to be relaxed to allow it.
   Recorded here because it rode in on a lease commit rather than arriving as
   its own change; `docs/nips/NIP-CSL.md` is updated.

33. **Two latent breaks of the same class: a Tauri test call site that no
   plain build compiles.** `desktop/src-tauri/src/commands/agent_discovery/relay_directory.rs:374`
   passed six arguments to a `build_create_channel` that grew a seventh
   (`project_ref`) in `5849a250e`. It sits behind
   `#[cfg(all(test, not(target_os = "windows")))]`, so only `cargo check
   --manifest-path desktop/src-tauri/Cargo.toml --tests` on macOS sees it.
   Fixed on `feature/project-containers`, the branch that introduced the
   parameter. Its sibling — `ChannelInfo`'s `project_ref` missing from
   `commands/channels_tests.rs:273` — is **fixed on the wrong branch**
   (`feature/builtin-shell` `bee7a76ff`), which is why the assembly is green
   while `feature/project-containers` and `feature/project-access` are not.
   Left alone deliberately: moving it now would collide with builtin-shell
   during the assembly merge for no product gain. Recorded in §3.

34. **`build/2026-08-20.7` never deployed: CI #105 exposed a provider
   state-lock release race.** The failing Rust step was not Redis Lua,
   Postgres, lease admission, or Pulse. Of 239 provider unit tests, the only
   failure was
   `state::tests::the_state_dir_lock_admits_exactly_one_holder`: immediately
   after dropping the first `StateDirLock`, the same process still received
   `WouldBlock` while reacquiring the temporary directory. The build escaped
   local ceremony because that run used `--skip-gate`; Woodpecker's exact
   workspace command caught it, and red pipelines do not autodeploy. This is
   why the subsequent lightyear read still returned zero provider-reachable
   sessions: lightyear was still serving the pre-24223 relay, so that read did
   not exercise lease behavior at all.

   `feature/coding-sessions` commit `6d8ec48d2` now gives `StateDirLock` an
   explicit `Drop` implementation that unlocks before the file descriptor is
   closed. The existing failing test was the regression seam; it passed 50
   consecutive focused runs after the fix. Both Woodpecker Rust commands then
   passed locally against fresh isolated Postgres 17, Redis 7 and MinIO,
   including the full workspace, all 239 provider tests and the separately
   selected mesh-demo test. Clippy, rustfmt and `git diff --check` are green.
   Deployment and the first meaningful live lease observation remain pending
   until this recovery build's Woodpecker pipeline and autodeploy are green.

### Found 2026-08-21 in Brian's first live turn — the prompt fence

35. **A fenced coding session had no way to know its fence was deliberate, so
   it reported the design as a broken install.** Asked to read its Project
   Pulse context and post an entry, a coding session answered: *"Project
   Pulse: unavailable — no `BUZZ_PRIVATE_KEY`/`BUZZ_RELAY_URL` is configured
   in this session's environment (checked `.env`, shell env, and the
   per-agent key files under `~/.config/buzz/`; all are empty placeholders),
   so I can't run `bee pulse digest` or `bee pulse update` at all."* It
   then chose **consult** rather than fabricating. The agent's behaviour was
   ideal; the product was wrong.
   - **What forbids it.** `crates/buzz-session-provider/src/agent_fence.rs`
     scrubs the entire `BUZZ_*` namespace from every adapter this provider
     spawns (`session.rs:558`, `spawn_with_env_fence`). Its own doc comment
     says the consequence is intended: the CLI "no longer authenticates from
     inside a coding-session agent's shell". So the session was right, and
     right for the designed reason — the empty key files were a red herring.
   - **The first diagnosis was wrong, and the correction is the finding.** It
     was believed that `crates/buzz-acp/src/base_prompt.md` was instructing
     the fenced adapter to run `bee pulse update`. A trace disproved that:
     `base_prompt.md` reaches only *managed* ACP agents, through
     `buzz-acp/src/lib.rs:2193-2198` → `PromptContext.base_prompt` →
     `pool.rs:966 framed_system_prompt`, and those agents are spawned
     `EnvFence::OPEN` (`acp.rs:557`) and can genuinely authenticate. The
     fenced adapter (`claude-agent-acp` via `buzz-session-provider`, spawned
     with `agent_fence::FENCE` at `session.rs:558`) received **no** Pulse
     instruction at all, and no `[Project Pulse]` digest either — that
     injection is `pool.rs:1663-1697`, buzz-acp only. The session was not
     mis-instructed, it was **un**-instructed: it hit a deliberate fence, had
     no way to know it was deliberate, and reported credentials as missing.
     Same dishonesty, better disguise.
   - **What the plan said.**
     `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md` §5.5,
     verbatim: *"Do not tell a fenced coding-session adapter to run `bee
     pulse update`."* — never violated, as it turns out — and *"Tell the
     adapter that its session state is visible automatically and that it need
     not post routine progress."* That second half is what was never built,
     and is what ships here.
   - **The fix.** `agent_fence::FENCED_SESSION_BRIEFING` (`agent_fence.rs:110`)
     states only facts about that process: `buzz` cannot authenticate here and
     the absence is the design, not a misconfiguration; the provider itself
     observes and publishes branch, `HEAD` commit, dirty state and verified
     liveness, so routine progress needs no post; no digest arrives here, and
     the way to learn what others are doing is to ask the operator
     in-conversation. It is delivered **unconditionally** on `session/new` via
     `_meta.systemPrompt.append` — fresh, rehydrated and restarted alike,
     because the fence applies to all of them — with the existing first-turn
     preamble as the fallback for adapters that have no `session/new`
     transport. A native `session/resume`/`session/load` reattachment takes
     neither: the conversation it restores already contains the briefing.
   - **Two false claims in the same paragraph of `base_prompt.md`, for the
     audience that *does* get it.** (a) It described the injected section as
     carrying *"entries only, never session state"* — true when written
     (`42f0443f2`, 2026-08-19) and false the next day, when `render_digest`
     grew provider-reachable / unverified / closed session groups
     (`60d7de756`, `pulse_fetch.rs:344-396`). An agent was being taught to
     discount evidence that was in front of it. (b) It asserted
     *"`BUZZ_PULSE_PROJECT` holds your current project's coordinate when one
     is in scope"* — the coordinate rides only on **MCP-server** env
     (`pool.rs:1104-1146 mcp_servers_with_git_origin`), never on the agent
     subprocess's own env, which is fixed at pool spawn and identical for
     every channel. An agent following that line runs `bee pulse update`
     with no `--project` and gets a usage error
     (`buzz-cli/src/commands/pulse.rs:397-422`). Both rewritten, and the
     prompt-injection guard widened to cover session text as well as entry
     text.
   - **Pinned by test.** `agent_fence::tests::the_fenced_briefing_never_tells
     _a_session_to_write_the_pulse` asserts both halves in one place — the
     fenced briefing contains no `bee pulse *` command and no
     `BUZZ_PULSE_PROJECT`, while `buzz_acp::BASE_PROMPT` still carries the
     write instruction its credentialed audience needs. Plus
     `session::tests::a_fresh_session_is_told_its_shell_is_fenced` and
     `shared_base_prompt_describes_the_pulse_section_it_actually_receives`.
   - **Also still unbuilt from the same §5.5 list.** Its first bullet — *"Add
     the same bounded digest to the context package"*, so a coding session can
     **read** the Pulse through the read-only context sidecar without any
     credential — has not landed: `buzz-dev-mcp`'s session-context tools serve
     the first-turn brief and history only. The briefing shipped here says
     "you will not receive a Project Pulse digest", which is true today and
     must be revised in the same change that lands that bullet.
   - **Open, deliberately not built today.** The fence's doc comment names
     the real answer — *"a coding-session execution that should speak to Buzz
     gets its **own** identity through `agent_ref`"* — and item 17 already
     tracks that convergence. Until it exists, a coding session cannot write
     the Pulse at all, and the honest prompt says so and points at the
     operator.
   - **Evidence, and its limit.** An independent verifier re-ran the gates on
     the delta: `cargo test -p buzz-acp -p buzz-session-provider` 1094 passed
     / 0 failed; `cargo clippy --workspace --all-targets -- -D warnings` exit
     0; `cargo fmt --all --check` exit 0; `just conformance-check` 47/47;
     `just file-size-check` exit 0. It also read the assembled prompt text and
     confirmed every claim the briefing makes is true of that environment
     (`EXEMPT` is empty and `PREFIXES` is `["BUZZ_"]` at
     `agent_fence.rs:70,52`; no digest path reaches it). **Still unproven end
     to end:** `build/2026-08-21.1` subsequently passed Woodpecker #109 and the
     deployed relay accepted leases, but **no live coding session has yet been
     observed receiving the briefing.** That provider/adapter observation is
     distinct from the relay proof and remains owed.
   - **How it was split.** `crates/buzz-session-provider/src/{agent_fence,
     session}.rs` plus `buzz-acp`'s new `pub const BASE_PROMPT` went to
     `feature/coding-sessions` (`e3cb42c5a`); the `base_prompt.md` rewrite and
     its test went to `feature/project-pulse` (`d6e56bb8e`), because the
     `## Project Pulse` section exists on no other branch. That leaves a
     **new, deliberate impurity**: the fence test reads a string introduced by
     `feature/project-pulse`, so it is red on `feature/coding-sessions`
     standalone and green on the assembly. Recorded in
     `docs/INTEGRATION.md`. Keeping the test whole was chosen over splitting
     six lines into glue; move it to glue if upstreaming that branch ever
     needs it.

36. **Agent Progress uses the lease; there is no second liveness clock.** The
   preserved sidebar work originally copied Pulse's deleted 30-minute
   freshness heuristic, so the same session could read `Working` there and
   `Open · liveness unverified` in Pulse. The rebuilt global
   `/agent-progress` preview route now consumes the same deep coordination
   fold as Pulse. That fold alone owns lifecycle authority, generation
   continuity, exact-generation lease selection, conservative expiry,
   closure, ambiguity and read completeness. The two surfaces are adapters
   over one result, not parallel implementations.

   The compact register is exactly `Reachable` / `Unverified` / `Closed`;
   reported provider status is a separate historical line and cannot establish
   reachability. Rows have fixed height, one row represents one umbrella
   session, and the footer counts distinct `executionKey` identities rather
   than resumed generations. It deliberately carries no token total because
   no source fact proves one. A partial read never says there are no agents and
   every nonzero count becomes an `at least` floor.

   The project shelf no longer makes a competing liveness claim: it says
   `Open sessions` and labels metadata as `Reported working` / `Reported idle`
   in neutral styling. Pulse and Agent Progress share the full status mapper,
   the coordination vocabulary, and the completeness envelope. The surface
   remains global rather than becoming a coding-session host tab because it
   coordinates across sessions; the separate N-tabs host decision does not
   block it.

   **Evidence before ceremony:** 6,009 desktop unit tests, 47/47 conformance,
   Agent Progress Playwright 2/2, TypeScript, Biome, px-text and file-size
   gates passed. Six inspected state screenshots were pairwise hash-distinct;
   they include reachable, unverified, closed, multi-execution and partial
   reads. Adversarial Standards and Spec re-review ended with no blocker or
   major.

   **The live 403s are an honest, coarse failure mode, not an authorization
   mismatch.** Durable session facts and lease snapshots use the same
   per-channel membership/access predicate; project ownership does not widen
   it. Their response semantics differ deliberately: durable multi-channel
   queries omit inaccessible branches, while an explicitly scoped lease
   snapshot fails if any requested `#h` is inaccessible, forcing the caller to
   mark the result partial. That prevents a falsely complete digest, but one
   revoked channel can make an entire 128-channel lease chunk unavailable and
   lower the observed floor. Adaptive split-on-403 is a recovery optimization,
   not permission to widen access.

### Recovered 2026-08-18 from superseded handoffs (verified still true)

These were tracked in documents that had been superseded and had fallen off
every live list. Each was re-verified against current source before landing
here.

12. **The agent inherits the operator's personal MCP servers.** The env fence
   scrubs the `BUZZ_*` namespace plus a few enumerated third-party secrets
   (`agent_fence.rs:52-60`) and says nothing about MCP configuration, so an
   adapter can reach an operator's personal Gmail/Calendar/browser servers.
   Env scrubbing cannot fix it. **This gates the collaboration release**
   (A6/A8): sharing a session must not share the founder's personal
   integrations. Originally SESSION_HANDOFF_SOL §7.3.
13. **Permission requests are auto-approved in the transport.**
    `session/request_permission` is answered `allow_once` inside the ACP read
    loop (`handle_permission_request`, `crates/buzz-acp/src/acp.rs:2194`; the
    option is found by `kind`, never a hardcoded id). No operator ever sees a
    prompt. **Observed live 2026-08-24**, dev provider log:
    `acp::permission: auto-approving permission id=0 with allow_once
    optionId="allow"`, immediately before the agent's tool call ran.
    **Asked directly by Brian on 2026-08-24 — "how is the permission setting
    being done in Buzz? Are we even handling that?" — and the answer is no.**
    t3code puts a four-mode control where Buzz has nothing
    (`CompactComposerControlsMenu.tsx:67-70`: Supervised / Auto-accept edits /
    Auto / Full access), which is the same vocabulary this item already
    recommended. Shipping that dropdown before the transport can honour it
    would be a control that lies about what it enforces, so what shipped
    instead is `CodingSessionAccessNotice`: a non-interactive row in the create
    and add-provider surfaces that states "Full access" and, on hover, that
    every tool call is approved automatically and there is nothing to choose
    yet. It is a disclosure, and it is where the control goes when this item is
    built.
    This is the root blocker for any approval story, and approvals appear
    nowhere in D1–D6. Prior art to reuse when it is picked up:
    a neutral runtime-mode vocabulary (`approval-required` /
    `auto-accept-edits` / `auto` / `full-access`) mapped per provider
    (SESSION_HANDOFF_SOL §6), and the architectural recommendation that
    session permission requests get their own signed kinds rather than
    reusing workflow kind 46010, while reusing Buzz's existing
    pending-decision / grant-deny / `needs_action` / push mechanics
    (SESSION_NATIVE_SUBSTRATE §1).
14. **A crashing adapter can never be classified auth-required.**
    `mentions_auth` inspects only `AgentError` message text
    (`session.rs:774`), so an adapter that exits *because it is not logged
    in* surfaces as generic `PROVIDER_UNAVAILABLE` and the guided-login
    affordance never appears. Andy hit exactly this. Compounded by **adapter
    stderr being inherited, not captured** (`acp.rs:649`), so the one
    diagnostic naming the cause never reaches a log a user can send.
15. **Execution labels collide.** Every execution row is labelled by the
    provider pubkey, so same-provider executions render identically. Assigned
    to UI Slice 2 but absent from the UI convergence handoff — it fell
    between two documents.
16. **Handoff carries two accepted v1 risks** (SESSION_STEP4_DESIGN): the
    quoted block is prefilled verbatim into another execution's
    operator-signed prompt (cross-agent prompt injection), and the
    "Handoff from X" chip is recognized from prose, not verified (label
    spoofing). Must be revisited before any autonomous/unmediated handoff —
    a named prerequisite of the persistent-agents step in D6.
17. **Managed-agent ↔ session convergence, concrete order** (SESSION_PATH):
    managed agents *into* sessions first via lane mention (no new wire
    concepts), then session executions *into* managed agents via `agentRef`
    (needs delegation grammar). D6 references the topic only abstractly.

### Found 2026-08-23 night — the first long live run on the dev instance

Six findings (items 39-44) from one session — "Testing1", in hallway, on the
Hallway project — driven from the keyring-mode dev instance against
`hive.agiterra.org`, with a second provider (Brian's prod app) owning some of
its executions. Items 37 and 38 were found earlier the same day; they are moved
here from the "Recovered 2026-08-18" block, where they had been misfiled and
where their numbers collided with items 18 and 19.

37. **`coding-sessions.spec.ts` "a seeded signed session is discoverable…"
    fails on `main`** (2026-08-23, run at `da3f181f`): the catalog entry no
    longer contains "Claude Agent Acp" — it renders
    "Fix the reconnect bug · Idle · Goal: … · Open". Either the entry dropped
    the provider name or the assertion is stale; decide which by reading the
    entry component, not by editing the test until it passes. Inherited, not
    caused by `fix/resumed-session-history`.
    - **Fixed 2026-08-24 by reading the component, as required.** The raw
      driver id was never coming back — the row's display name became the
      session's name. What the assertion was really protecting is that the row
      names its agent, and it had stopped doing that, so a single-provider row
      now carries the human label ("Claude Code") beside the name and the spec
      asserts that. A **second** stale assertion of the same class was hiding
      behind the first, never reached because it failed earlier: the authority
      gate's copy became the roster-aware "View only — ask the session owner
      for collaborator access" when grant/revoke landed (`8bb80ff7`).
      `coding-sessions.spec.ts` is green — 7 smoke tests.

38. **The catalog trigger counts generations, not sessions.** A resumed
    session reads "Coding sessions (2)" and lists two entries that open the
    same umbrella. `desktop/tests/e2e/coding-sessions.spec.ts` ("a resumed
    session renders every earlier generation") asserts that real count on
    purpose; the fix must update the spec in the same change.
    - **Fixed 2026-08-24.** `resolveChannelCodingSessionIngress` groups through
      the umbrella model and emits one row per durable session, opening its
      most recently active generation. Each row discloses what it stands for —
      "2 providers", "2 generations" — so collapsing hides nothing. Both e2e
      assertions were updated in the same change, and the multi-provider case
      collapsed from two rows to one for the same reason.

39. **Codex offers exactly one model, named `default`, and every Codex
    execution is then labelled with it.** Seen in "Add provider → Codex":
    one option, `default`, where Claude lists real model ids; the execution
    row then reads `Codex · default`. This is the "default label hiding the
    real model" case `AGENTS.md` names as a first-class bug.
    - **Where the placeholder comes from.** Both production construction
      sites hardcode it: `desktop/src-tauri/src/session_provider/runtimes.rs:159-160`
      (the descriptor list written to `BUZZ_CSP_RUNTIMES`) and `:219-220`
      (the picker row). Discovery is what is supposed to replace it, and
      only `claude` opts in — `runtimes.rs:49` vs `:58`, with codex declared
      at `:53-58` and goose at `:61-68`.
    - **Two discovery implementations, and only one of them is
      driver-agnostic.** The sidecar's
      `crates/buzz-session-provider/src/model_catalog.rs:24` iterates every
      descriptor with `discover_models` and probes it through ACP
      (`extract_model_config_options` / `extract_model_state`) — genuinely
      driver-agnostic. The **desktop** command the picker calls is not:
      `desktop/src-tauri/src/session_provider/commands.rs:71-110` returns the
      static `"default"` for any runtime with `discover_models: false`
      (`:82-88`) and its discovery branch resolves `claude-agent-acp` /
      `claude-code-acp` by name (`:91-94`), with error strings to match. So
      flipping codex's flag alone would make the picker probe **Claude's**
      adapter and report Claude's models under Codex. The desktop branch must
      take the runtime's own `adapter_commands`/`agent_args` first.
    - **Correction to the handoff.** It cited
      `crates/buzz-session-provider/src/commands.rs:602` as a production
      fallback; that line is inside `#[cfg(test)] mod tests` (the module
      begins at `:584`) and is a test fixture. The production placeholders are
      the four `runtimes.rs`/desktop-`commands.rs` sites above.
    - **The label is a second, separable bug.** An execution must display the
      model the adapter *reported* (`extract_model_state`), not the string the
      create requested — for every driver, including Claude.
    - **Fixed 2026-08-24, both halves.** codex opted into discovery after the
      probe was verified against it (`buzz-acp models --json` →
      `currentValue: gpt-5.6-terra` plus the full option list); goose stays out
      because the same probe answers `-32603 Internal error`, tested rather
      than assumed. The desktop probe stopped being Claude-hardcoded:
      `runtime_probe_target` resolves each runtime's own adapter and args, and
      the runtime's name reaches its errors. And `apply_model` now falls back
      to `buzz_acp::acp::reported_model` — stable `configOptions.currentValue`
      first, then unstable `models.currentModelId` — so an unofferable request
      publishes what the adapter says it is running instead of the request.
      The recording fake agent grew a `MCP_TEST_MODELS` hook and answers
      `session/set_config_option`, so both branches run against a real ACP
      exchange; dropping the fallback turns the unofferable-model test red.

40. **Codex says the session-context MCP is not among its callable tools while
    the transcript tells the operator that verified history is available.**
    Both halves are in one file: the dev provider log
    `~/Library/Application Support/io.agiterra.beekeeper.app.dev/session-provider/logs/1958c6c4….log`.
    The Claude execution calls it — `tool_call:
    mcp__buzz-session-context__session_overview` at line 110 (01:47:40Z). The
    Codex execution on the same umbrella shows one `execute` (line 181,
    01:49:55Z, a `sed` over `docs/STATUS.md`) and then, at 01:49:59Z (lines
    281-284, streamed one token per line, which is why a plain `grep` for the
    sentence finds nothing): *"The session-context MCP isn't currently exposed
    among my callable tools, so I can't refresh the provider's snapshot beyond
    the briefing you supplied."* The UI meanwhile rendered the continuity
    marker *"Rehydrated — verified session history is available to this
    agent"*.
    - **Not a provider-side omission, as far as the seam goes.**
      `crates/buzz-session-provider/src/session.rs:680` builds `mcp_servers`
      once (from `rehydration_mcp_servers`, `:801`) and passes it to
      `session_new_full` (`:710`, `:774`), `session_resume_full` (`:728`) and
      `session_load_full` (`:750`) alike — no driver branch. Suspect
      `@agentclientprotocol/codex-acp` ignores `mcpServers` on `session/new`
      and wants the server in the generated `CODEX_CONFIG`
      (`crates/buzz-acp/src/config.rs:739-790`, merge contract at
      `crates/buzz-acp/src/acp.rs:261-316`).
    - **A negative result proves nothing here.** codex-acp answers
      unrecognized extension methods with `{}` — a JSON-RPC *success*, not
      `-32601` (`crates/buzz-acp/src/acp.rs:200-207`, which is why
      `steering_supported` exists as an explicit capability gate).
    - **The premise above is wrong, and the investigation is the finding
      (2026-08-24).** Brian's instruction was *do not work around this — fix
      it*, so the transport was traced to the wire instead. Every layer is
      healthy. Codex's own log proves it for the **live** thread:
      `~/.codex/logs_2.sqlite` id 643510, 01:49:29Z, from
      `app_server.client_name="buzz-acp"` — `session_init.mcp_manager_init:
      mcp.runtime.refresh:new{server_name=buzz-session-context}: Service
      initialized as client`, carrying the sidecar's own `serverInfo`
      (`buzz-session-context-mcp 0.1.0`) and its instructions text. The server
      was attached, spawned and handshook for the execution that said it had
      no such tool.
    - **What is actually different about codex.** Reproduced offline against
      the same adapter (`@agentclientprotocol/codex-acp` 1.6.2), the same
      `codex` 0.148.0, the same model settings the live turn used
      (`gpt-5.6-terra`, effort `Low`) and that execution's own context
      package: codex does **not** put MCP tools in the model's function list.
      Asked to name every tool it can call, the model lists only
      `functions.exec`, `functions.wait`, `functions.request_user_input` and
      the `collaboration.*` set. The MCP tools live on codex's *code-execution*
      surface — callable as `tools.mcp__buzz_session_context__session_overview()`
      inside the sandbox, displayed on tool rows as
      `mcp.buzz-session-context.<tool>`. Called that way they work: the proxy
      capture shows `tools/call` reaching the sidecar and the real package
      coming back. So "the MCP isn't currently exposed among my callable
      tools" is *literally true of the function list* and false as an
      operational claim — the honest-looking sentence that sent this
      investigation after a phantom.
    - **Ruled out, each by test, not by reading.** codex-acp forwards
      `mcpServers` for every driver (`session.rs:680-774`); the `{command,
      args, env}` entry it emits matches codex's stdio schema; injecting the
      same server through `CODEX_CONFIG` instead behaves identically;
      `features.tool_search`, `features.code_mode` and `tools.code_mode`
      change nothing; and Buzz's exact `clientCapabilities` (no `fs`, no
      `terminal`) make no difference. `/mcp` listing the server with no tool
      count is a red herring — `mcpServerStatus/list` is config-scoped and
      never sees a session-injected server.
    - **What shipped.** `context_tool_access_note`
      (`crates/buzz-session-provider/src/session.rs`) appends one
      codex-only sentence to the rehydration briefing naming the code-mode
      identifiers, so the adapter is told where its tools actually are instead
      of being pointed at a function name it does not have. Claude's briefing
      is unchanged — the note would be false there — and both halves are
      pinned by `a_codex_bootstrap_names_the_code_mode_path_and_a_claude_one
      _does_not` plus assertions in the two transport tests; disabling the
      note turns two tests red.
    - **Closed live, 2026-08-24 03:12-03:13Z, and the fix is not what closed
      it.** Brian drove a Codex execution on the dev instance against hive.
      Provider log, same file as the original finding: session opened
      `03:12:19` (`continuity=Rehydrated bootstrap_transport=Some(FirstTurn)`,
      followed by the codex model WARN), then
      `03:12:52.793636Z tool_call: mcp.buzz-session-context.session_overview
      (execute)` and `03:13:44.194656Z tool_call:
      mcp.buzz-session-context.session_history (execute)`. The agent answered
      both questions from verified history — including "the first user message
      was ping" — and the continuity marker's claim was true.
      **That app predates the briefing note**, so the note is not why it
      worked: the tools were always reachable, and eight offline runs with the
      live package and model settings called them with and without the note.
      What remains of this item is only the false self-report the note targets
      — a Codex execution asked *whether* it has the MCP still answers from its
      function list, where MCP tools never appear. The live failure of
      2026-08-23 was never reproduced and no longer has a suspected mechanism
      beyond that.
    - **Two things this run also showed.** The Codex tool rows arrive as kind
      `execute`, not as tool calls (relevant to item 43's row rendering), and
      the header still read `Coding session idle` under a live, answering
      provider — item 41, seen from the other side.

41. **The session header reads Idle over a provider that is not running.**
    Brian's prod app quit at 21:17 (its `session-provider` log: `shutdown
    requested`) without publishing `disconnected`. For the next two hours the
    Testing1 header showed **IDLE** and the composer offered Send and Stop
    execution. Nothing could answer either one.
    - **Cause.** `deriveCodingSessionWorkspaceStatus`
      (`desktop/src/features/coding-sessions/lib/codingSessionWorkspaceModel.ts:220`)
      takes exactly three inputs — the transcript, the newest signed 44223
      `status`, and its `statusAt`. The kind-24223 lease is not among them, so
      "the last thing this provider said" is rendered as "what is true now".
    - **The honest model already exists one feature over.**
      `agentLaneReportedText`
      (`desktop/src/features/agent-progress/lib/agentProgressFormat.ts:37`)
      states the status plainly only when `coordination ===
      "provider_reachable"`, and otherwise says *last reported* with an age —
      the exact distinction this header erases. That is §2 item 36's fold, and
      §3's "do not build a third fold" applies.
    - **Fix.** Feed lease reachability into the workspace status; with no live
      lease the header reads "Last reported Idle · 2h ago", Send is disabled
      with the reason "no provider is currently answering for this execution",
      and Stop is replaced by the affordance in item 42. Wants a unit test
      with a stale lease and an e2e case seeding metadata `running` with no
      lease event.
    - **Fixed 2026-08-24.** `deriveCodingSessionWorkspaceStatus` takes a
      reachability verdict and demotes a live-sounding status when coordination
      proves nobody is answering: the header reads `No provider answering ·
      last reported Idle 2h ago`, the composer disables its editor and says
      why, and the reported status is kept as history. Signed terminal and
      attention states outrank it. The verdict comes from the shared
      coordination fold through `useCodingSessionReachabilityResolver`,
      resolved once per surface and threaded down, so a workspace with eight
      executions still makes one read — no second liveness clock (§3). Fail
      open everywhere: a missing read, a partial read and an unproven
      generation all demote nothing. Both tests exist —
      `coding-session-reachability.spec.ts` seeds the same two-hour-stale
      session twice and differs only in whether an unexpired lease exists, and
      neutralising the demotion turns it red (the mutation had to compile:
      a `tsc` failure makes Playwright serve the previous `dist` and the run
      lies, §3a).

42. **Stop is terminal, nothing says so, and stops aimed at a dead provider
    vanish instead of queueing visibly.** Brian pressed Stop three times on
    the unreachable execution; each time the dialog confirmed and nothing
    happened. When the prod app returned at 21:43 its provider drained all
    three (`session stopped ×3`, one per command id).
    - **The product is right and the copy is wrong.** A `stopped` execution is
      deliberately not resumable: the composer offers **Reconnect** only for
      `disconnected` and renders "This provider execution has ended." for
      `stopped`
      (`desktop/src/features/coding-sessions/ui/CodingSessionComposer.tsx:155-157`,
      `:345-380`). The confirm dialog
      (`hooks/useEndCodingSessionDialog.tsx:85-88`) says the durable session
      and transcript stay open — true — but never says the execution cannot be
      revived, so the operator expects a Resume button that will never appear.
    - **Fix.** (a) Dialog: "This execution cannot be resumed. To continue the
      work, add a provider to the session." (b) The ended banner gets an **Add
      provider** action. (c) A command with no reachable provider surfaces as
      pending — "Stop requested; no provider is listening" — and the same for
      turns. `useCodingSessionResumeSettle` already implements the
      refusal/receipt watcher for resume; extend the pattern rather than
      inventing a second one.
    - **Fixed 2026-08-24 (a, b, and c for stop).** The confirm says a stopped
      execution cannot be resumed and names the way forward; the ended banner
      carries an **Add provider** action, as does the unanswered banner from
      item 41. A stop aimed at a provider that is not answering is described as
      what it is — the dialog says it stays on the relay and runs whenever one
      returns, and the receipt after publishing says "Stop requested — no
      provider is listening". Only that case speaks; a delivered stop shows
      itself in the transcript. The copy moved into
      `endCodingSessionDialogDescription`, a pure function, so both branches
      are pinned by tests rather than by reading JSX. **Turns are covered by
      item 41's disabled composer rather than by a pending receipt** — a turn
      that cannot be sent is better than a turn that queues invisibly — so the
      "same for turns" half is deliberately not built.

43. **A Codex tool row elides the shell binary as private context, and the row
    becomes unreadable for no privacy gain.** Rendered: `Ran [elided private
    context: 10 bytes, sha256:…] -lc "sed -n …"`. The ten bytes are the
    interpreter path.
    - **The guard is doing exactly what it was built to do.**
      `sanitize_coding_session_context_text`
      (`crates/buzz-core/src/coding_session_context.rs:869`) redacts
      **per word**, and `contains_host_path` (`:887-920`) matches any absolute
      unix path — so `/bin/zsh` is elided and the rest of the argv is kept
      byte-for-byte. Nothing is mis-scoped in the predicate.
    - **The collision is with §2 item 2**: `codex-acp` leaves `tool.input`
      empty and puts the whole command in `tool.toolName` as prose, so a field
      that is prose for Claude is argv for Codex and meets a redactor written
      for prose. The fix belongs at that seam — parse the command structurally
      for codex, or exempt an interpreter `argv[0]` — not by loosening the
      host-path rule, which is protecting real host layout.
    - **Fixed 2026-08-24 with the second option.** An exact-match allowlist of
      stock POSIX interpreters (`/bin/sh`, `/bin/zsh`, `/usr/bin/env`, …) is
      exempt from `contains_host_path`: those strings are identical on every
      host and disclose nothing. Nothing else moves — `/usr/local/bin/zsh`,
      `/opt/homebrew/bin/bash`, `/bin/zsh-custom` and anything under `/Users`
      stay redacted, and an exempt argv[0] does not rescue the host paths
      beside it in the same command. Three tests, watched fail.

44. **Coding-session lane messages also appear in the ordinary channel
    timeline — but not for the reason recorded here (corrected 2026-08-24).**
    The finding said `publishCodingSessionLaneRenderableRefs`
    (`desktop/src/features/messages/lib/codingSessionLaneVisibility.ts:104`)
    had no production caller. It has had one since 2026-08-18:
    `useCodingSessionLaneVisibility`
    (`desktop/src/features/messages/useCodingSessionLaneVisibility.ts`, added by
    `a84f7b01` "hide session lane chat only where it is renderable"), mounted
    from `useUnreadChannels:142` so every channel gets the same answer, not
    only the visible one. There is nothing to wire.
    - **What is actually true.** Suppression is deliberately narrow and
      fail-open in two ways, either of which produces what was seen. (a) It
      applies only to umbrellas that *render* a lane —
      `umbrellaHasCollapsedHistory`
      (`codingSessionUmbrellaModel.ts:156`): more than one execution, or one
      carrying prior generations. A single-execution session that has never
      been resumed shows no lane, so hiding its tagged chat would hide it
      everywhere, and it stays in the timeline **by design**. (b) Until a ref
      resolves through the global catalog subscription, the message is ordinary
      chat and counts toward unread — the rule that stops a forged
      `cs-session` tag from hiding a message from every new client.
    - **What would close it.** A reproduction that names which umbrella the
      message belonged to and whether that umbrella renders a lane. If it does
      and the message still shows twice, the defect is in resolution timing and
      `refreshChannelWindowMessages` is the seam; if it does not, the product
      is behaving as designed and the item is a documentation fix, not a code
      one. Not changed on speculation.

### Found 2026-08-24 in the first live pass on the fixed build

Brian rebuilt the dev instance on the honesty-pass commits and drove Testing1
against hive. Item 41 was confirmed working. Four new findings, three fixed the
same morning and one left as a product question.

45. **The session header read ENDED over four live executions.** Testing1
    showed **ENDED** beside "5 executions" while one was answering turns.
    Provider log: the newest execution (`16e36197`) was stopped at 11:04:36Z,
    and `deriveUmbrellaStatus`
    (`desktop/src/features/coding-sessions/lib/codingSessionUmbrellaModel.ts`)
    fell through to "the most recently active execution's status" — so one
    per-execution stop ended the whole umbrella on screen. Same class as item
    41: a fact about one part rendered as a fact about the whole.
    - **Fixed.** Activity still outranks quiet and quiet outranks terminal, but
      the fall-through now picks the newest execution that has *not* ended.
      Only an umbrella whose every execution stopped reads ended.

46. **Two composer chips nobody could tell apart.** `Send to Codex · default ·
    1958c6c4…9644`, twice. The disambiguator appends the *signer*, which
    separates executions across providers and says nothing within one.
    - **Fixed** by falling through to the head of each execution's own
      provider-minted session id when the signer still collides.
    - **Not a bug, and worth remembering:** those two chips really do say
      `default`. They are executions created before item 39 landed, and their
      signed metadata says `default` because that is what the provider
      published at the time. History is not rewritten; only new executions
      carry the reported model.

47. **One dropdown for two decisions.** With discovery on, Codex's model list
    became thirty rows — `gpt-5.4`, `gpt-5.4[low]`, `gpt-5.4[medium]`,
    `gpt-5.4[high]`, `gpt-5.4[xhigh]`, and the same for five other families —
    because codex encodes reasoning effort in the model id. Brian: "this needs
    to be fixed so that the 'thinking' level is a separate dropdown."
    - **Fixed** with `codingSessionModelChoice.ts`: Model and Thinking are
      separate controls over the same published ids, recombining to exactly the
      id the adapter listed. Two rules keep it honest — `opus[1m]` does **not**
      split, because that bracket is a context window and splitting it would
      invent a choice `claude-agent-acp` never offered; and "Adapter default"
      is offered only where the adapter also published the bare id, because for
      a level-only model it would name something the provider would refuse.
    - **The first picker was wrong in ten ways, and Brian named the shape he
      wanted: provider/model → thinking → access.** The critique that produced
      the current one, worth keeping because most of it generalises: the ⌘N
      hints were **rendered with nothing listening for them** — a keyboard
      shortcut that was only a picture of one, which is this project's own
      favourite class of bug; rows showed wire ids (`claude-fable-5[1m]`)
      instead of names; `default` sat among the models as if it were one;
      `[1m]` stayed glued to the name; every row repeated the rail it was in;
      the panel was 26rem and landed on top of the dialog's own fields; the
      selected row had a tint but no checkmark; and the trigger repeated the
      provider its own glyph already showed.
    - **Then the picker itself, which Brian asked for next**, modelled on
      t3code's (`~/Projects/t3code/…/components/chat/ModelPickerSidebar.tsx`,
      `modelPickerSearch.ts`) and rebuilt on Buzz primitives: provider and
      model are one control with a provider rail, favourites, search, and ⌘N
      hints, and Thinking stays its own control beside it. Favourites are
      local-only and provider-scoped — pinning this machine's Codex must not
      pin a catalog entry from another machine — and a provider whose catalog
      has no models keeps a row, because a signed-out runtime publishes none
      and dropping it hid the row that explains why it cannot be used.
    - **Two bugs the rebuild found in itself, both by e2e rather than by
      reading.** Starring a model made it jump out from under the cursor,
      because an unsearched list was still sorted by favourite — pins now
      collect on the Favorites rail and never reorder a provider's own list.
      And **Escape did not close the panel**: the dismissable layer never saw
      the key, so the picker handles it directly. A keyboard-driven list a
      person cannot leave with Escape is a trap, and a static render cannot
      catch either of these — `coding-session-model-picker.spec.ts` presses
      the keys.
    - **What the adapter really publishes**, worth knowing before trusting
      either list: the *stable* `configOptions[category=model]` carries 7 clean
      base models (probed 2026-08-24), while the unstable `availableModels`
      carries the effort combinations, and the desktop merges both into one
      "unified model list, deduplicated by ID"
      (`managed_agents/types.rs:742`). Both shapes reach the UI; the fold is
      what makes them one list again.

48. **OPEN, product decision: should a stop be a pause?** Brian, after stopping
    a Codex execution: *"why can it no longer be resumed? Should there be a
    pause and a stop?"*
    - **Half of it was naming, and that half is fixed.** The composer carried
      two adjacent buttons both reading **Stop** — one interrupting the current
      turn, one ending the execution forever. The turn-level control is now
      **Interrupt** everywhere (the immersive deck already called it that).
    - **The rest is a real question.** A stopped execution is terminal by
      ruling R27, and the composer offers Reconnect only for `disconnected`.
      Nothing technical forbids resuming one: `session/resume` reattaches by
      cursor, which is exactly what the disconnected path already does. A
      **pause** would be a deliberate move into that same reconnectable state —
      release the provider process, keep the execution resumable — and it needs
      a new lifecycle action on the wire (command, receipt, ACL, provider
      handling), not a UI change. Do not build it without Brian choosing
      between "stop stays terminal and Add provider is the way forward" and
      "pause becomes a first-class lifecycle state".


### Found 2026-08-24 in Andy's session on the shared relay

49. **A coding session working on authorization could not show its work.** Andy
    asked "what was the result?" and received `[elided private context: 1934
    bytes, sha256:…]` — the whole answer, replaced by a hash. Three of his
    turns rendered that way.
    - **Cause.** `contains_credential_material`
      (`crates/buzz-core/src/coding_session_context.rs`) substring-matched a
      list of *words* — "secret", "credential", "authorization", "private key",
      "token:" — and any hit replaced the **entire** text with an elision
      marker. His session was on `feat/project-membership-git-acl`. This repo
      ships a binary called `git-credential-nostr`; an agent explaining a push
      cannot avoid the word. The guard was keyed to the topic, so the sessions
      most worth reading were the ones it erased.
    - **Fixed the same day: secrets are redacted, sentences are not.** A hit
      now has to be a *value* — a PEM block (redacted whole), a token with a
      recognisable shape (`nsec1…`, `sk-…`, `ghp_…`, `github_pat_…`, `xoxb-…`,
      `AKIA…`), or the value side of an assignment (`token=…`, `password: …`,
      `api key is …`, and the bare-space form when the following token looks
      like a value rather than the next word of a sentence). A bare mention of
      a credential word is prose and survives. Every shape the old rule caught
      is still caught; the projector's own leak test proved it, and the leak
      direction is pinned by tests that were watched fail.
    - **The tradeoff, stated.** This narrows a guard on signed, channel-visible
      text. The old rule leaked nothing and destroyed everything; the new one
      keeps the message and can in principle miss a secret written in a shape
      no rule anticipated. That is a deliberate trade, and the shape list is
      the place to add to when a new one appears.

50. **Two of the same session's turns were killed as "Idle timeout — no agent
    activity for 900s".** The clock is a *silence* budget, not a runtime
    budget: every line the adapter writes resets it
    (`crates/buzz-acp/src/acp.rs`, the read loop's `idle_deadline`). A single
    long command that reports only on completion — a build, a test suite, this
    repo's own ~11-minute `just ci` — spends the whole fifteen minutes without
    the agent being stuck at all.
    - **Fixed by making it the person's.** Settings → App → Sessions carries a
      "give up on a silent turn after" control beside the session ceiling,
      stored the same way and disclosed the same way: it reaches the provider
      at its next start, and the panel says so while the two differ. The
      default is unchanged at fifteen minutes, because raising it silently
      would trade one wrong number for another.
    - **Amended 2026-08-25, re-amended 2026-08-26.** Some of these turns were
      not slow at all — see item 59. A silence budget cannot distinguish work
      in progress from a prompt the adapter dropped, and both were landing
      here. Two of the six turns in the measured session were the latter.

51. **A sent turn took two round trips to appear, so the composer looked
    broken.** "There is a lag prior to seeing my messages appear on the screen
    — I'm thinking because this is to the relay and back?" It is longer than
    that: a transcript renders signed facts, and the user's own message is a
    kind-44225 `user_prompt` published by the **provider**. The path is sign →
    44220 → relay → provider receives and starts the turn → provider publishes
    the item → relay → verify → project → render. Unbounded when the provider
    is busy or reconnecting.
    - **Fixed** by `codingSessionPendingTurns.ts` plus
      `CodingSessionPendingTurns.tsx`: the editor empties before any await
      (restoring the draft if the publish fails), and the turn shows
      immediately in a row that lives **outside** the signed narrative —
      the umbrella timeline's invariant is that facts from different signers
      never merge, and a row nobody has signed is not one of those facts.
    - **Settling has no id to key on.** A signed prompt carries content and
      operator, not the command id that caused it, so a pending record is
      consumed by the first signed prompt with matching text and operator — one
      record per arriving item, so the same sentence sent twice settles in
      order. A failed publish removes the row, restores the draft, and puts the
      relay's error by the composer; silence expires after three minutes.
      Registered in `resetCommunityState()`.
    - **The row says nothing about itself.** The first version spun a loader
      and said "waiting for the provider to start the turn"; both true, both
      the wrong frame — they made the moment about our plumbing when the person
      is waiting on an agent to think. It now speaks only where silence would
      lie: a failed publish, and a turn nobody picked up after ten seconds
      ("Not picked up yet"). Nothing claims the model is thinking, because
      nothing knows that.

52. **The full-screen session needs a design pass. The critique, so it is not
    re-derived** (from a screenshot, 2026-08-25, ordered by severity):
    1. *The composer's gradient overlay leaks the transcript through it* —
       `via-background/85` in `CodingSessionUmbrellaWorkspace.tsx:274` renders
       text at 15% behind the chip row, which reads as overlapping content
       rather than a fade.
    2. *A raw, double-escaped JSON blob opens the transcript* — an MCP tool
       result whose `content[0].text` is itself a JSON string, `\n` literals
       and all, occupying ~40% of the first screen. Wants a one-line summary
       ("Session history · 47 items") expanding to parsed content.
    3. *Two thirds of a 3456px window is empty.* The 48rem measure is right for
       prose and matches t3code — but t3code fills the flanks. Agents, Observed
       changes and People are built surfaces hiding behind header buttons; at
       this width they should be open beside the transcript.
    4. *Raw identifiers the picker already fixed elsewhere* —
       `mcp.buzz-session-context.session_history` as a tool title,
       `gpt-5.6-terra[low]` in chips and the composer footer. The same session
       says it both ways.
    5. *`0.0s` on every tool call*, which reads as "did not run".
    6. *The same fact three times*: header `IDLE`, composer footer `Idle`,
       transcript "Coding session idle".
    7. *Two chip systems forty pixels apart* — `⇄ Send to Claude · …`
       mid-transcript and the participant row above the composer.
    8. *An empty goal is the largest element on screen.*
    9. *Turn boundaries are invisible* — user bubble, reasoning, tools and
       completion flow together with only the cost line separating them.
    10. *`Send` and `Stop execution` are neighbours* at the same weight, one of
        them terminal.

    **Implemented on `fix/full-screen-session-ux` (2026-08-25):** the composer
    dock is opaque with its fade entirely above it; the narrative and composer
    expand from 48rem to 72rem only while no Agents/Changes surface is open;
    the latest signed plan now drives both the compact inline plan + Work Log
    and a T3-style Tasks sheet attached above the composer instead of a side
    rail. The empty goal is a small action. Turn boundaries have separators.
    Session-context MCP results unwrap their one-text-block transport envelope
    and summarize as `Session history · N items`; exact-zero durations are
    omitted. Packed model ids are decoded everywhere the session labels them
    (`gpt-5.6-terra · Low`, not `gpt-5.6-terra[low]`). The composer no longer
    repeats the header's Idle/Working state, completed-turn handoff chips are
    revealed only on hover/focus, and terminal execution stop is an icon action
    rather than a peer of Send. Evidence: the focused desktop tests plus the
    three-view Playwright workflow in
    `coding-session-transcript-narrative-screenshots.spec.ts`; its collapsed
    task, expanded task, and opened Changes views are pixel-distinct.

53. **A running turn no longer locks the full-screen editor** (live comparison
    with T3 Code, 2026-08-25). The old composer disabled its textarea whenever
    the provider did not declare `threadSteer`, conflating “cannot inject into
    this turn” with “cannot write the next one.” The editor now stays available:
    a steer-capable provider still offers `Steer`; otherwise the action reads
    `Queue`, stores the draft locally without publishing a command or optimistic
    row, and publishes exactly once when the current turn settles. The queued
    row is explicit and cancellable. `CodingSessionComposer.optimistic.test.mjs`
    proves the relay sees zero commands while the turn is working and one after
    the state becomes idle; the wide E2E screenshot proves the editor is enabled
    in that state.

54. **OPEN: Claude background shell completed, but the following ACP prompt
    never resolved until Bee Keeper cancelled it.** Andy's shared session is
    channel `7df9fd91-0066-461c-bc6b-5f49c6bb9a16`, session
    `ecde2480-c334-491d-ad6f-c8685e22ee02`, generation 1, signed projection
    `4b6fd011e70b1323…c150f4312b9` (`claude-agent-acp`, title "Rebuild Bee
    Keeper"). The signed kind-44225 sequence separates two failures that look
    like one spinner:
    - Turn 1 launched `scripts/local-prod-build.sh HEAD` as a Claude Terminal
      background command. Its tool result explicitly said "You will be
      notified when it completes" (event seq 20), but the ACP turn then ended
      successfully after 41,422 ms (seq 27). That promise is not a Bee Keeper
      capability: a detached shell can outlive the prompt, and neither ACP nor
      the adapter creates a new person-visible turn when it exits. The current
      adapter's background-subagent hold deliberately excludes background
      shells because a server can live forever; its own fix records this as an
      out-of-scope, out-of-turn episode
      ([claude-agent-acp #870](https://github.com/agentclientprotocol/claude-agent-acp/commit/7a70f82739e085014cad878f08513cdef7b7fe16)).
    - The build itself was healthy. When Andy asked "Is it done?" 41m 50s
      later, Claude immediately emitted two Terminal calls and two successful
      results (seq 29-32); the captured build output says `Finished release`,
      bundle OK, installed, exit code 0. Then the adapter emitted **nothing for
      934,726 ms**. At Bee Keeper's configured 900s silence boundary the
      provider sent `session/cancel`; only during that cancellation drain did
      Claude flush the complete "Yes — done" answer (seq 33), followed 19 ms
      later by the honest terminal result `Idle timeout — no agent activity
      for 900s` (seq 35). This is the exact wire shape independently reported
      upstream: streamed output, no response to `session/prompt`, response only
      after `session/cancel`
      ([claude-agent-acp #970](https://github.com/agentclientprotocol/claude-agent-acp/issues/970)).
    - A second upstream Claude SDK report now names the preceding trigger:
      background-task notifications can sit queued in streaming-input mode
      until a later user message wakes the session
      ([claude-code #88378](https://github.com/anthropics/claude-code/issues/88378)).
      That is consistent with this session, but the signed transcript does not
      carry Andy's installed adapter/SDK versions or provider stderr, so it is
      an inference, not yet the proven local root cause.
    - **Do not "fix" this by only raising the silence budget.** That merely
      moves an unresolved ACP request farther away; Bee Keeper's timeout and
      cancellation did the useful thing here and preserved both the late answer
      and the fact that its turn failed. Next evidence: on Andy's machine record
      `claude-agent-acp` and Claude Code versions plus raw ACP/provider stderr,
      then reproduce once on the latest released adapter. Product follow-up:
      never let an agent promise a proactive report for a detached shell unless
      the session has a real session-level background-work lifecycle to deliver
      it.

55. **The active plan and composer now behave as one turn-scoped work surface**
    (T3 Code comparison, 2026-08-25). The floating composer is a solid surface
    with one borderless editor and one quiet control row: the provider/model
    identity and access label open explanatory popovers; model traits are
    informational rather than fake selectors; execution stop is terminal and
    lives under More; and a context meter appears only when a signed
    `Context Window Updated` item supplies real token use. Send/Queue/Steer and
    interrupt retain the existing authority and capability gates.
    - The Tasks attachment is derived only from the newest signed plan in the
      currently running transcript turn. A stale, untraceable, completed, or
      idle plan cannot pin itself above the composer. Closing dismisses that
      turn's attachment; a newer signed turn may open its own. Completed rows
      retain elapsed time derived from same-turn signed plan snapshots, and the
      active row reads `now`. Completion releases the attachment automatically.
    - The Goal bar, transcript, task attachment, and composer share one measure:
      72rem while the workspace is clear, 48rem while Agents or Observed changes
      occupies the side, with both values rem-based so Cmd +/- preserves the
      reading measure. The Goal bar now lives inside the shrinking narrative
      section rather than remaining centered across the hidden flank.
    - Multi-provider umbrellas use that same surface instead of leaving their
      routing chips and instructions above it. The recipient is a compact
      human-labelled control inside the composer; its menu keeps execution
      targets visible but honestly disabled without control authority, and the
      Session lane remains available. Selecting an execution also selects the
      signed active Tasks attachment for that execution. Umbrellas now open
      with no side surface, then contract only when Agents, Changes, or People
      is requested.
    - Evidence: 47 focused composer/task/context tests; all 6,166 desktop unit
      tests; the three-view transcript narrative screenshot workflow; and the
    width workflow at 1100/1280/1920/2560/3440px, with a side surface and at
    24px root zoom. The repository-wide `just ci` gate passed on 2026-08-25.

56. **A multi-agent session now reads as one attributed story, not one
    agent's log with the others hidden behind a count** (T3 Code-informed pass,
    2026-08-25). The signed flat timeline remains the record; presentation now
    makes each execution legible without splitting it into tabs or swimlanes.
    - Every execution has a stable accent used by its header chip, turn rail,
      sticky provenance, and handoff actions. Sticky provenance exists only in
      the merged multi-agent read, where the author can otherwise scroll away;
      it is bounded by its turn block and uses an opaque surface.
    - The header execution chips are the focus control. Selecting Codex folds
      other agents' turns to attributed one-line summaries in their original
      positions; people, handoffs, and lifecycle facts are never hidden.
      Selecting the chip again returns to All. Focus and the composer's
      recipient deliberately share identity styling but no state, so reading
      Claude cannot retarget a draft and choosing a recipient cannot hide the
      passage being handed off.
    - The T3-style **Active Work** attachment is the current work surface for
      every execution that is actually working. It shows only a current signed
      plan when one exists, says truthfully when no plan was published, and
      collapses to nothing when all executions are idle or complete. Dismissal
      is scoped to the current work fingerprint; newer work can reopen it.
    - A completed turn ends with an always-visible boundary naming its author,
      signed duration/cost when supplied, and explicit Reply / Send-to actions.
      Assistant prose is back on the app's `text-base` chat ramp. The goal is
      under the session title, founder provenance moved into Info, and the
      Agents / Changes / People controls form one responsive surface switcher.
    - Multi-agent sessions automatically open Agents beside the narrative only
      when the available body is at least 1920px. Laptops and single-agent
      sessions keep the clean full-width transcript; narrow layouts compact
      the header and retain the execution chips below the goal.
    - Workspace-contained absolute paths are relativized before the provider
      signs transcript context. Anything still private remains fail-closed but
      renders as a compact `Private context · N bytes` chip whose tooltip keeps
      the digest, rather than shredding the answer with inline hashes.
    - Evidence: 6,175 desktop unit tests; 31 focused core sanitizer tests plus
      the provider signing-boundary test; the seven-session E2E workflow and
      the two-view surface-host workflow, including focus without recipient
      mutation and 2560px Agents auto-open; all captured states are pixel-
    distinct. The repository-wide `just ci` gate passed on 2026-08-25.

57. **The multi-agent controls now preserve the story instead of competing
    with it** (T3 Code comparison plus the four-execution UXV1 transcript,
    2026-08-25). The crowded row of one chip per execution and a second
    aggregate status badge is one compact `N agents · M working` control. Its
    popover is the place to focus an execution or open full agent detail; the
    working state has a restrained breathing accent derived from signed
    activity, so the session feels alive without inventing progress.
    - Focus remains a reading mode, never a routing mode. Choosing an agent
      shows a small `Viewing …` notice above the narrative with an explicit
      release action, leaves the composer recipient unchanged, and scrolls the
      filtered transcript to its latest content instead of stranding the
      reader near the first matching turn.
    - The composer makes routing persistent and explicit as `Send to …`, with
      the same execution accent and the participant's exact disambiguated
      label. Completed turns now keep only `Reply` plus one `Send to…` menu;
      empty folded turns no longer manufacture a generic `Signed execution
      activity` row.
    - T3's moving-highlight treatment is adapted for `Thinking`, but its truth
      boundary is Bee Keeper's: it appears only after a signed running turn
      exists and before any signed plan, tool, answer, or error becomes visible.
      The optimistic unsent row remains silent. Breathing and text-sweep
      animation both become static under reduced motion.
    - Evidence: 62 focused component tests; both focused surface-host E2Es;
      the five-test coding-session E2E; and nine pixel-distinct medium, narrow,
      focused, surface-open, and ultrawide screenshots. The focus E2E starts at
      the top, selects Codex, proves the viewport is within 8px of the bottom,
      and proves the composer recipient did not change. The repository-wide
      `just ci` gate passed on 2026-08-25: 6,177 desktop tests, 2,681 Tauri
      tests, every mobile test, all workspace tests, and desktop/web production
      builds are green.


58. **Turn receipts and commandId on the echo (Slice 1).** Built overnight
    2026-08-25/26 on `crew/s1-truthful-turns@5f7ce22d`, not landed. On the
    wire, kind 44224 gains four statuses — `turn_queued` (mailbox accept),
    `turn_started` (`run_turn`), `turn_dropped` (queue overflow, code
    `QUEUE_FULL`), `turn_refused` (every `TurnDecision::Fail` plus the
    `Ignore` reasons that name a target) — in the existing exact-key shape,
    with `turnId` present only on `turn_started`, and the semantic key
    `coding-session-lifecycle-receipt/v1|<len>:<commandId><len>:<status>`
    (byte-pinned in `crates/buzz-sdk/src/builders.rs:6371`). Kind 44225
    `user_prompt` items now carry the turn's `commandId`, so the desktop
    settles an optimistic row by id and falls back to the text join only for
    echoes without one — and says so on the row (`data-settled-by="text"`).
    Evidence: `just ci` green to completion (desktop 6172/0, mobile 1465, every
    Rust/Tauri suite ok — log `/tmp/crew-s1-ci-round3.log`; the overnight runner
    reported before it finished, so the first record read red), `just test`
    exit 0 with 223 integration tests passing, `pnpm check:px-text` exit 0.
    Two test-only hitchhikers: pair-relay `test_cancellation_immediate` (a
    flake — passes on unpatched main) and the naming-settings test made
    keychain-free (it hung the Tauri step in a worktree).
    Residuals: nothing ran against a relay or a live provider (the
    relay-backed e2e is `#[ignore]`d, the app was never opened), so the Rust
    and desktop halves have not met on the wire; `DeliverError::Gone` and the
    no-live-actor arm still eat a turn with only a `tracing::warn`; the
    mailbox-full drop receipt is untested; a `turn_queued` row still expires
    at the 3-minute pending TTL and the suppressed stall escalation is now
    pinned by a test; the Rust decoder's closed four-code `turn_refused` list
    will reject S6's `BUDGET_EXHAUSTED` while the desktop accepts it.

59. **A coding session hung for fifteen minutes after it had already
    answered.** Andy: the answer finishes, "but nothing triggers the main
    thread to run again". Confirmed against the live transcript — see the
    measurement bullet below, which also **rules out the subagent cause this
    entry originally named**. What follows about `claude-agent-acp` 0.70.0
    (`~/Library/Application Support/Bee Keeper/node-tools/lib/node_modules/
    @agentclientprotocol/claude-agent-acp/dist/acp-agent.js`) is a real latent
    bug found while reading for this, but it is **not** what happened here:
    `settleOrDefer` (`:1559-1575`) holds the `session/prompt` open
    while a Task subagent is live, and when the subagent ends —
    `task_notification` at `:2339`, terminal `task_updated` at `:2350` — the
    entry is deleted but **`settleDeferredIfDrained` is never called from any
    task-lifecycle handler**. It is reachable from `:2106`, `:2138`, `:2714`
    and nowhere else. The turn is drainable and nobody looks. The adapter's own
    idle-without-result failsafe (`#825`, `:2141`) cannot rescue it either:
    `isHeldOpen` (`:124-127`) tests only "outcome recorded, not yet resolved"
    and sits ahead of it in the idle chain, so a held turn always
    short-circuits first. Per the bundle, only a client-sent `session/cancel`
    or a new prompt gets out. A session that *does* spawn a subagent should
    therefore hang exactly this way; none of the transcripts read so far does.
    - **Measured 2026-08-26 against the session Andy named, and the subagent
      cause did not survive it.** `bee sessions transcript` on channel
      `023cf12c-e589-4771-88d4-99b0a2d68cec`, session
      `d9e04173-3b78-4c00-9891-269ac4eab643` generation 1 ("project repo
      access"): 582 items, 6 prompts, **2 stalled** — turn `150fcad7` at
      2026-08-24 12:09:01Z and turn `7f0bb8af` at 13:15:49Z, both closed by the
      idle guard at 952.1s and 954.1s. The check this ledger set for itself was
      "an unterminated Task-shaped `tool_call` plus a ~900s gap confirms the
      hold; a terminated one points elsewhere." It pointed elsewhere:
      **257 `tool_call`s, 257 `tool_result`s, zero unterminated, and no
      Task/Agent call anywhere in the session** (census: Terminal 207, Edit 30,
      Read File 8, ToolSearch 4, one each of EnterPlanMode, grep, Monitor,
      TaskStop, ListAgents). The adapter cannot have been holding: `:2333` adds
      to `spawnedTaskIds` only when `message.subagent_type` is set, and
      `turnAwaitingSubagents` requires `record.isSubagent` (`:1538`) — the
      bundle calls this the "shells-never-defer contract" in as many words.
    - **What the transcript does prove is the shape, and the fix keys on the
      shape.** Both stalled turns end `assistant_text` → `context_window_updated`
      → `result` at one instant — the close flush — exactly like the four clean
      turns; the difference is only that their `result` is Buzz's synthesized
      error carrying `inputTokens: 0, outputTokens: 0` where a clean turn
      carries real counts. **No SDK `result` ever arrived.** Subtracting the
      900s budget from each span locates when the wire actually went quiet.
      **Corrected 2026-08-26 against the provider log, which logs the timeouts
      directly and made the arithmetic unnecessary:** turn `150fcad7`'s idle
      fired at 12:24:53, +952s, so its last frame was at +52.1s. Turn
      `7f0bb8af`'s fired at 13:31:13, **+924s not +954s** — the extra 30s is
      the cancel-drain grace, logged separately as `hard turn timeout exceeded
      (silence 30.0015465s)` at 13:31:43 because that adapter ignored the
      cancel. Its last frame was at **+24.1s**, not the +54.1s first derived.
      Subtracting the budget from the span only works when the drain is clean.
      Either way: a short burst of streaming after the last tool, then nothing. The
      answer had finished and the adapter went silent — which is precisely what
      `BUZZ_CSP_ANSWER_STALL_TIMEOUT` arms on. Both turns would have been
      nudged at ~172s and published Completed with the answer intact, about
      **780 seconds earlier**.
    - **The strongest remaining lead is background work, not subagents.** A
      backgrounded terminal command (`btt3f5d4c`, armed 11:43:22Z, never
      stopped) and a `Monitor` armed 11:43:27Z were live across the stalls, and
      the Monitor's own timeout is **900000ms — the same 900s as Buzz's idle
      budget**, so the two clocks are indistinguishable by duration alone. It
      is not deterministic: the turn between the two stalls ("Ok to push.")
      completed in 149s with the same background task live. Deciding this needs
      the adapter stderr and raw SDK frames that item 59's own step 1 now
      captures and that were not running on 2026-08-24 — so the next stall,
      not this one, is the one that will name the cause.
    - **The earlier reading was wrong, and worth recording as wrong.** The
      first analysis held that T3 Code stays healthy because it keeps the SDK
      stream alive across turns while the ACP adapter does not, and proposed
      forking the adapter on that basis. Both run a single persistent
      streaming-input `query()` per session — T3 at `ClaudeAdapter.ts:3855-3863`
      and `:4377`, the adapter at `acp-agent.js:~1002-1036`. The real
      difference is that **T3 never lets background work block a turn**:
      `ThreadBackgroundLiveness.ts` is an advisory sidebar pill. ACP has to
      answer "which result settles which prompt", and chose to wait.
    - **Also amend the claim that the adapter "flushed its answer only during
      cancellation".** That timestamp is Bee Keeper's own flush. The translator
      buffers agent text to a size, tool, or turn-end boundary
      (`transcript.rs:13-19`, `:122-127`) and `close_turn()` runs after the
      cancel drain.
    - **Fixed by watching for the shape of a finished turn.** Once top-level
      prose has streamed and every tool call opened has reported terminal,
      nothing is left to be doing and the result normally follows in
      milliseconds; silence *there* gets `BUZZ_CSP_ANSWER_STALL_TIMEOUT`
      (default 120s) instead of the fifteen-minute silence budget, which stays
      as it is because it still has to cover genuinely long tools. On expiry
      the provider **nudges** with the adapter's own escape hatch rather than
      killing: the turn usually returns a real stop reason and is published
      **Completed with the answer intact**, plus a status row saying Bee Keeper
      had to close it. Reporting it as clean would hide the defect from the
      only person able to report it upstream.
    - **A second, independent silence, now closed.** The adapter strips a
      subagent's prose and thinking unless the client declares the
      `subagent-transcript` capability (`:3252-3259`, `:4768-4769`), and Buzz
      did not. A subagent that reasoned for minutes without calling a tool put
      *nothing* on the wire, so the idle timer counted down through healthy
      work. Buzz declares it now — and the translator had to learn to ignore
      `_meta.claudeCode.parentToolUseId` in the same change, or the subagent's
      narration concatenates into the agent's own prose as one indistinguishable
      block. Publishing subagent work properly needs an item kind the clients
      can render; until that exists the honest handling is to carry the
      liveness and publish nothing.
    - **Still open, deliberately.** Nothing reads the adapter's stdout between
      turns (`session.rs`, the actor's idle select has no reader arm), so a late
      notification waits for the next prompt and past ~64 KiB the adapter blocks
      on write. That is the out-of-turn delivery channel T3 has and Buzz does
      not; it is prerequisite for ever promising "you'll be notified when it's
      done", and it will not fix this hang on its own.
    - **Instrumented 2026-08-26 so the next stall names its own cause.** Six
      changes, because the forensic pass showed the evidence was not merely
      missing but structurally unreachable.
      1. **The desktop app installed no `tracing` subscriber**, so its own
         instrumentation went nowhere; `desktop/src-tauri/src/logging.rs` now
         opens a daily rolling file under
         `~/Library/Logs/io.agiterra.beekeeper.app`. **Corrected same day —
         this was first written up as far more important than it is.** The
         claim was that the ACP and provider stacks' ~440 call sites were all
         being discarded. They were not: `buzz-session-provider` runs as a
         supervised *child process*, installs its own subscriber
         (`buzz-session-provider/src/lib.rs:147`), and has its stdio
         redirected by `supervisor.rs:621-629` to
         `<app data>/session-provider/logs/<pubkey>.log` — a file that already
         existed and is where `acp::stall`, `acp::stderr`, `acp::sdk_frame`
         and every `csp::` line actually land. The desktop crate has 14
         tracing sites of its own, not 440. **Read the provider log, not the
         desktop one, when diagnosing a session.**
      2. **The timeout errors carry a `TurnWireSummary`** (`acp.rs`): frames,
         bytes, kinds, first/last arrival, tools in flight, whether prose had
         streamed. The message now says "last agent_message_chunk at +52.1s;
         quiet 900.0s; 0 tools in flight; answer streamed" — the sentence that
         cost an hour of subtracting the budget from the span by hand.
      3. **`BUZZ_CSP_IDLE_TIMEOUT` defaults to 870, not 900.** Claude Code's
         `Monitor` budget is 900000ms; while ours matched, a turn killed at
         ~900s could have been ended by either clock and the duration did not
         say which. Do not round it back.
      4. **Abnormal turns publish the same facts as a `turn_wire:` status
         row**, so a reader who was never near the machine has them too.
         Abnormal only: every turn is published and kept forever.
      5. **`BUZZ_CSP_EMIT_RAW_SDK_FRAMES`** asks the adapter for every raw SDK
         message (`acp-agent.js:5183`, forwarded at `:1829-1836`). These carry
         `origin.kind` and the whole task lifecycle. **Local log only** — they
         are the adapter's unredacted internals, and a test drives a frame
         carrying a host path and a credential marker through a live session
         and fails if either reaches a published item.
      6. **`bee sessions doctor`** does the triage as a command. It separates
         the two failures that look alike: zero usage with every tool
         terminated is a prompt the agent never resolved; real usage with a
         call still open is a hung tool. Where no `turn_wire` row exists it
         says the quiet window is inferred rather than measured.
    - **2026-08-26, first raw frames: a better upstream bug, with evidence.**
      `BUZZ_CSP_EMIT_RAW_SDK_FRAMES=true` on the rebuilt app produced 8447
      frames in two hours, and among them the task lifecycle this whole
      investigation was missing. A backgrounded Task subagent (`Explore`,
      task `a6522ff1683ff2069`, 10:06:59) emits a `task_started` carrying
      `task_type:"local_agent"` and `is_backgrounded:true` — and **no
      `subagent_type` field at all**. `subagent_type:"Explore"` appears on the
      `task_progress` frames instead, which the adapter discards with a bare
      `break` (`:2307-2308`).
      The adapter reads exactly the field that is absent, on exactly the frame
      that lacks it: `isSubagent: !!message.subagent_type` (`:2330`) and the
      `spawnedTaskIds.add()` gate (`:2333`). So `isSubagent` is false, the set
      stays empty, `turnAwaitingSubagents` returns false — **the deferred-settle
      hold cannot arm at all on this CLI (2.1.241)**. It is not a missing
      drain call; it is a field-name mismatch upstream of it.
      This supersedes the `settleDeferredIfDrained` reading as the thing to
      report: same subsystem, direct frame-level evidence, and it explains why
      no transcript here ever reproduced the hold. The observed instance did
      not hang — the agent answered from the progress frames at 10:09:09, one
      second before the subagent completed, and the turn ran on to a result at
      10:22:32 — so this is a latent defect, not the cause of the 08-24 stalls.
      16 of the other 17 tasks were `local_bash`/`is_backgrounded:false`, which
      correctly never defer.
    - **2026-08-26, built: background work is visible now.** The lived
      complaint turned out to be a different defect from the stall, and the one
      with evidence. A session launches an async subagent or a detached build,
      the turn ends, and nothing reports on it — the agent's "I'll tell you when
      the tests finish" was a promise nothing here could keep.
      - `BUZZ_CSP_EMIT_RAW_SDK_FRAMES` is a **mode**, defaulting to a filtered
        `lifecycle` set. Unfiltered cost 8447 frames in two hours (6072 of them
        `stream_event`) for the 80 that carry information; `shouldEmitRawMessage`
        takes a matcher array, so the filter is a whitelist. `all` is the
        firehose, `off` sends no `_meta` key at all. Default-on is the point:
        the adapter emits nothing about the task lifecycle to the wire, so
        these frames are the *only* channel that says background work exists.
      - `BackgroundTasks` folds them into structural facts. Classification
        reads `task_type`, **not** `subagent_type` — the latter is the field
        whose absence breaks the adapter's own hold. Registration requires
        `is_backgrounded`, because 16 of the 17 tasks observed were ordinary
        foreground commands the turn was already waiting on.
      - The actor's between-turns select gained a reader arm. It folds raw
        frames and **buffers everything else** for the next turn's read loop,
        which dispatches it exactly as before — buffering rather than dropping
        is what keeps this behaviour-preserving for the frames it does not
        interpret. Out-of-turn permission requests still wait for the next turn.
      - Rows publish with `turnId: null`;
        `codingSessionTranscriptModel.ts:102` already renders those standalone,
        so no synthetic turn was needed. Counts, kinds and durations only —
        `description` and `prompt` are verbatim host paths and argv, and
        `fit_item` is a size cap, not a scrubber.
      - The idle reaper no longer fires over live work, bounded at two hours,
        and says so when the bound is hit. Reclaiming a session whose build is
        still running kills the build.
      - **No new `SessionStatus` variant**, deliberately: the enum has no
        `#[serde(other)]` fallback, so an unknown variant makes the whole
        metadata payload fail to parse in every Rust consumer including `bee`.
    - **Not yet reported upstream, and the report must not overclaim.** The
      missing `settleDeferredIfDrained` call in the two task-lifecycle handlers
      is a one-line fix in their code and a genuine latent bug, but no
      transcript here reproduces it. File it as a code reading, not as an
      incident report.


### Built 2026-08-24 — project membership is the repository access signal

**Landed on `main` 2026-08-24; not yet deployed.**

Adding or importing a repository used to demand an "access channel", because
the `buzz-channel` tag on the kind:30617 announcement was the only git ACL the
relay knew. Projects already had a role-carrying roster (NIP-MP kinds
9010/9011 → `project_acl_members`, `owner`/`collaborator`/`viewer`) and repos
already declared their container (`["project", …]` → `git_repo_names
.project_ref`), so the channel was a second, parallel ACL the user had to keep
in sync by hand.

The roster is now a first-class git ACL, **additive** to the channel binding:

- `buzz_db::project_acl::get_project_role_by_coordinate` — the git-ACL lookup,
  deliberately **without** the `visibility = 'private'` clause its neighbours
  carry, because those decide whether to *hide an event surface* and this one
  decides whether a roster *grants*. Do not unify them.
- `api/git/policy.rs` step 7 resolves project and channel roles independently
  and takes the more permissive (`git_perms::max_git_role`); owner → Owner,
  collaborator → Member, viewer → no push (`git_perms::
  git_role_for_project_role`). `buzz-protect` rules are unchanged.
- `api/git/transport.rs` `authorize_git_read` admits **any** roster role at
  **any** visibility, and the `bee repos bind` remediation body is now scoped
  to repos with neither tag — a repo inside a project is legitimately unbound.
- Desktop: the "Access channel" `<select>` is gone from the create, import,
  and legacy add-repository dialogs, replaced by `ProjectRepoAccessNote`.
  `accessChannelId` is optional everywhere; the legacy add-repo path now emits
  the `project` back-reference it never had (without it those repos would have
  become reachable by nobody).
- CLI: `bee repos create --project`, `bee repos bind --project`, `--channel`
  now optional on both.

**Two decisions worth not re-litigating.** (1) Project *visibility* never
grants git access — a public project's repos are no more cloneable than a
private one's, or every repo sitting in the auto-created public `general`
project would have silently become community-readable. (2) The channel binding
still works and still grants, so no existing repo lost access and no migration
was needed.

**Deliberately left open.** For a *public* project the repo's kind:30617,
relay-signed 30618 ref state, and NIP-34 patches (kind 1617 — which carry full
diffs) are already community-readable, while `git clone` is not. Decision (1)
keeps that gap rather than widening clone access to match it. It predates this
change; it is recorded here because this is the first time anyone looked
straight at it.

Evidence: relay gate tests against Postgres —
`api::git::transport::sec005_read_gate_tests` 12/12 (5 new, including
"public project's repo is still not cloneable by a non-member") and
`api::git::policy::tests` 5/5 (4 new). Live over real git against a local
relay: `e2e_git` 3/3 including the new
`git_access_follows_the_project_roster_without_a_channel_binding` — owner and
collaborator clone and push a channel-less repo, a viewer clones but cannot
push, a stranger is refused, and no identity is a member of any channel.
`e2e_repo_visibility` 6/6 and `e2e_project_roles` 3/3 unregressed. `just ci`
green (desktop 6015/6015). The `scripts/e2e-git-perms.sh` roster phase is
written and `bash -n` clean but **was not executed** — that harness needs
`websocket-client` and wants port 3000.

### Found 2026-08-25 — the responsiveness pass, and the UI critique behind it

## 2a. Direction settled 2026-08-18

Three independent answers to "what should a new execution get on its first
turn" — two blind design reviews and Sol's implementation — converged on: push
a small brief the agent would not know to ask for, pull the rest, never inject
raw transcripts. Sol's shipped answer goes further and emits only what the
record already proves (turn outcomes, tool attempt outcomes, cited event ids)
rather than a model-written summary, because a generated summary manufactures
a new unverified claim. That is the shipped direction. Distillation of older
material is still worth building, but it must cite those evidence ids rather
than replace them, and it is not started.

## 3. Next — one track at a time, in this order

**Direction set 2026-08-25: crew sessions.** Executions become agent seats
with roles that address each other durably, a lead seat dispatches, and the
human founder observes. The plan, its operating model (lead / lanes / refuter
/ finalizer), the verified seams, and the six slices live in
[docs/CREW_SESSIONS_PLAN.md](CREW_SESSIONS_PLAN.md); its section 7 ledger is
where slice status goes, findings still come here. The amas predecessor's kit
is at ~/Projects/amas (read-only; contains keys). **Slice 1 (turn commandId on
the echo + per-stage receipts) was built overnight by a Claude-only crew on
crew/s1-truthful-turns@5f7ce22d**: gate green — `just ci` ran every recipe to completion (desktop 6172/0, mobile 1465, all Rust and Tauri suites ok; the runner reported early, the log proves the finish) and the lead's `just test` on 4b49249b passed 12/12 sections, 223 integration tests, 0 failed;
refuters NOT-REFUTED (contract & runtime correctness) and NOT-REFUTED (test
honesty and evidence), both same-family and therefore advisory, not the
cross-family pass §1 requires for a tier-2 diff. Residuals: nothing was ever
run against a relay or a live provider — the relay-backed e2e is `#[ignore]`d
and the app was not opened, so the Rust and desktop halves have never met on
the wire; `DeliverError::Gone` and the no-live-actor arm still consume the
command and drop the turn with only a `tracing::warn`, because the locked
contract gives `turn_dropped` exactly one code (QUEUE_FULL); the mailbox-full
`turn_dropped` path is implemented but untested (the actor mailbox could not
be forced full deterministically); a row the provider signed `turn_queued` for
still vanishes at the 3-minute pending TTL, and the lane's own test now pins
the suppressed stall escalation as intended; a refused `Ignore` does not
consume its command, so a redelivered 44220 republishes a byte-identical
refusal (duplicate signal, deduped by `csl-key` downstream); interrupts get no
stage receipts at all; the Rust receipt decoder pins `turn_refused` to four
codes while the desktop accepts any bounded code, which will bite when S6's
`BUDGET_EXHAUSTED` ships; `bee sessions list --json` reports `malformed: 0`
for a malformed turn receipt because of a raw-JSON fallback; the TESTING.md
live-observation block (§6.13.1) is marked NOT YET RUN LIVE; and three
out-of-lane or shape deviations are recorded in the lane reports (two
mechanical `turn_id: None` lines in
`crates/buzz-db/src/coding_session_generation.rs`, the semantic-key helper
placed in `buzz-sdk/src/builders.rs` rather than beside its siblings, and
`CodingSessionProjectedTranscriptItem` as a coding-sessions-owned alias
because `TranscriptItem` lives in the agents lane). It is a topic branch
awaiting Brian's live look; not landed on main.

**The active track as of 2026-08-25 night is the full-screen UI/UX pass — §2
item 52's ten points, in that order.** The first four are contained (the
composer's leaking overlay, the raw JSON tool result, the flanking dead space,
and the raw identifiers the picker already fixed elsewhere); the rest are
hierarchy and action-weight work. Everything below is the previous track, kept
because its live confirmations are still owed.

**The previous track was the coding-session honesty pass — §2 items 37-44. As of
2026-08-24 the code is done and the live confirmation is not.** Six items are
fixed behind tests, item 40 is closed by a live tool call on hive, item 44's
premise was corrected. **What is owed is one pass on the dev instance against
hive with these commits built in** (`env -u BUZZ_DESKTOP_NOKEYRING just
desktop-standalone`, ≈5 min after a cargo change): add a Codex provider and
confirm the picker lists real model ids and the execution row stops saying
`default` (39); quit the app that owns an execution and confirm the header
says "No provider answering · last reported …" and the composer explains
itself (41); stop that execution and confirm the receipt says the stop is
queued (42); and read one Codex shell row for its command (43). Until that
happens these are tested, not proven. It jumps this queue. The numbered list below is
unchanged and resumes after it; its numbers are referenced by
`REHYDRATION_HARDENING_IMPLEMENTATION_PLAN_2026-08-19.md` and
`PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md`, so do not
renumber it.

**The order the honesty pass is verified in, cheapest question first.** Each
step answers a different question and none of them substitutes for the next.
(1) `pnpm exec tsc --noEmit`, `pnpm test`, `pnpm exec biome check` from
`desktop/`, plus `cargo test -p buzz-session-provider -p buzz-acp` — seconds.
(2) `cd desktop && pnpm test:e2e:smoke` — this is where a UI claim is proven,
also seconds; `coding-sessions.spec.ts` already seeds signed relay events
through `__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__` and has genesis/create/metadata/
transcript helpers and a two-generation case to copy. **Prove the test bites**:
turn the fix off, rebuild, watch it go red, turn it back on — a test that was
never red proves nothing. (3) `env -u BUZZ_DESKTOP_NOKEYRING just
desktop-standalone` against hive, with Brian driving the UI and the agent
reading the provider log and `bee --format compact sessions list --channel
d244ad0a-d51d-4a2c-b98d-41795c5e9ca3` — minutes, and the only step that can
answer an adapter question like item 40. (4) `just prod-desktop` **only** when
Brian wants his daily driver updated: it is a 20-minute release build that
installs over the running app, and using it to verify a UI change is what cost
an hour on 2026-08-23 (§3a). (5) Land on a topic branch — `git commit -s`, `git
rebase --signoff origin/main`, `just check`, `git push --force-with-lease`,
fast-forward `main` — and **do not push to `main` while Brian is mid-test**: a
green push makes `beekeeper-autodeploy.timer` restart the hive relay, even for
a docs-only commit.

1. **Re-test the duplicate create** (§2 item 1) on this build. The
   single-provider-instance lock shipped in `build/2026-08-18.12`; nobody has
   yet created a session on it and confirmed one click makes one execution.
   Until that is done, item 1 is suspected-fixed, not fixed.
2. ~~**Rehydration hardening** — §2 items 4–6 as a single bite.~~ **The code
   shipped 2026-08-20 in `build/2026-08-20.7`** (§2 items 4, 5, 6 and item
   1's disclosure half). What is left is live confirmation, not code, and it
   is three specific runs: (a) a two-execution session where a sibling
   advances and the joined execution is checked for the *later* work after a
   turn boundary — refresh is turn-triggered and floored at 60 s, so a
   too-quick check proves nothing; (b) a resumed execution, confirming it now
   reports `no_umbrella_context` and not `no_prior_execution` (item 3's code
   half shipped in `b9de9a6d` and still wants this same run); (c) a
   >200-item session paged by `since` cursor across a refresh, confirming the
   cursor survives it and `stoppedBy` reads true.
3. **Verify Andy's People/roles flow** with a second identity
   (viewer → operator → revoke). It plausibly closes §2 items 7–8; do not
   strike them on commit messages alone.
4. **Finish removing metadata-derived liveness from coding-session detail**
   (§2 items 20 and 36). Agent Progress now reads the lease, and the project
   shelf was reduced to neutral `Reported …` history, so neither competes with
   Pulse. Audit the remaining coding-session detail/workspace surface and
   either give it the shared coordination result or make every status claim
   explicitly historical. Do not build a third fold.
5. **Move `ChannelInfo`'s `project_ref: None` to its owning branch** (§2 item
   33). It is on `feature/builtin-shell`; it belongs on
   `feature/project-containers`. The assembly is green either way, but two
   feature branches do not compile under `--tests` until it moves, and the
   split map cannot be executed cleanly around it.
6. **Step 0b of the split map — the `buzz-core` Pulse/coding-session
   impurity.** `pulse_fold.rs` now takes seven coding-session imports on top
   of the one `pulse.rs` already had, so `feature/project-pulse` does not
   compile standalone. Same class as before, deeper.
7. **P1**, whenever an hour exists. It gates the entire seed/checkpoint
   track; everything downstream in the research report §7 is speculation
   until it runs.


The four rehydration defects (§2 items 3–6) are one subsystem and one
coherent bite. Then 7–8 to make multi-member testing self-service. P1 whenever an hour
exists. Longer horizon lives in the research report §7: relay-durable
checkpoints (kind 44231) and encrypted native-snapshot sync (44232).

## 3a. Environment facts that cost real time (do not rediscover)

- **The dev instance on this machine must run in keyring mode, whatever the
  docs say.** `docs/local-desktop-instances.md` recommends
  `BUZZ_DESKTOP_NOKEYRING=1` and `~/.zshrc:6` exports it, but the dev instance
  was migrated *into* the keychain on 2026-08-22: the identity, six agent keys
  and the hive provider key live in the `beekeeper-desktop-dev` blob and the
  JSON records have their inline keys stripped. In file mode the app mints a
  throwaway identity and every agent and provider comes up key-less, which
  reads as a broken build. Launch it as `env -u BUZZ_DESKTOP_NOKEYRING just
  desktop-standalone`. The doc still needs fixing.
- **`pkill -f "tauri dev"` leaves vite alive** holding `BUZZ_VITE_PORT`, and
  the next launch dies on "Port 13946 is already in use" while looking like a
  compile failure. `lsof -ti:13946 | xargs kill` before relaunching.
- **Prod and dev share one human identity but not one provider, and an
  execution belongs to the provider that created it.** Human `3d3b7169…`,
  prod provider `3728312c…`, dev provider `1958c6c4…`. Quitting the app that
  owns an execution leaves nobody able to answer it — turns and stops sit in
  the relay until that provider returns (§2 items 41 and 42, where three stops
  drained two hours later). When a session stops responding, check *which*
  provider owns it before debugging the code.
- **Markdown is not automatically inert, and `docs/` is not automatically
  documentation.** Two files defeat the obvious CI path filters:
  `crates/buzz-acp/src/base_prompt.md` is `include_str!`'d into `BASE_PROMPT`,
  and `docs/nips/NIP-MP.fixtures.json` is `include_str!`'d by
  `crates/buzz-sdk/src/builders.rs:5419`. So `**/*.md` and `docs/**` are both
  unsafe as exclusions — each would skip the gate, and therefore the relay
  rebuild, for a change to compiled output. The live exclusion is `*.md`,
  `docs/*.md`, `docs/**/*.md`, guarded by
  `scripts/test-woodpecker-path-filter.sh`, which found the `docs/**` case
  before it shipped.
- **Woodpecker's push path filter sees the whole push here, not just the tip
  commit** — contradicting its own documentation, which says "only files from
  the most recent commit". Pipeline 324 (`a961fb277`) recorded `changed_files`
  spanning all six commits of a push tipped by a docs-only one. This is what
  makes the docs-only filter safe for multi-commit pushes; re-check it if the
  forge or Woodpecker major version changes, because the documented behaviour
  would silently mask a relay change behind a docs commit.

- **`CLAUDE.md` is a symlink to `AGENTS.md`.** Editing through it modifies
  `AGENTS.md`; `git add CLAUDE.md` then stages an unchanged symlink and drops
  the edit with no error. On 2026-08-23 a branching-policy change committed as
  "1 file changed" and had to be amended — the commit stat was the only tell.
  Stage `AGENTS.md`.
- **`just desktop-standalone` reaps its own successor.** The recipe traps
  `cleanup-instance-agents.sh "$INSTANCE_ID"` on EXIT, and two runs on the same
  branch share that id, so a dying run kills the replacement — `Terminated: 15`,
  exit 143, seconds after a clean-looking start, which reads as a build failure.
  Let the old run finish dying before launching. Related: for testing against a
  real community use `desktop-standalone`, not `just dev` — `dev` boots a local
  relay and a worktree identity, so hive.agiterra.org data is simply absent.
  `productName` and the bundled feature manifest are fixed at launch/build time;
  a running app cannot pick up either, only a relaunch can.
- **Renaming the desktop crate breaks three things that fail silently.**
  `buzz-desktop` → `beekeeper-desktop` on 2026-08-23 (the dock label is the
  Cargo binary name, not `productName`). `desktop-release-cache-key.py` matched
  the old name in `Cargo.lock`; `local-prod-build.sh` pgreps the binary to
  refuse installing over a running app — its own comment records that guard
  silently passing through the *previous* rename; and `DESKTOP_BINARY_NAMES`
  drives orphan-agent reaping, so the new names were **added**, not substituted,
  because installed older builds still run under the old one. Deliberately not
  renamed: `buzz-desktop-latest` (the GitHub release tag inside
  `BUZZ_UPDATER_ENDPOINT` — renaming breaks auto-update for every install) and
  `BUZZ_DEV_KEYRING_SERVICE` (keys stored dev secrets).
- **Autodeploy is healthy — a relay image that looks stale probably is not.**
  On 2026-08-23 hive ran `beekeeper-relay:7d224a8b6` while local `main` was at
  `a45b25397`, which read as the deployer's documented quiet-failure mode. It
  was the opposite: `origin/main` had advanced and `7d224a8b6` was *newer*.
  Compare against `origin/main` after a fetch, not against a local branch.

- **Three remotes, and the names moved on 2026-08-24 — check, do not
  remember.** Current: `origin` = the relay's own git hosting
  (`hive.agiterra.org/git/<owner>/agiterra-beekeeper`, needs Nostr
  credentials), `upstream` = `agiterra/beekeeper` on GitHub where `main` lives
  and Woodpecker watches, `vanilla` = `agiterra/buzz` (the block/buzz mirror
  plus the one CI patch). Merge vanilla with
  `git fetch vanilla && git merge vanilla/main`. There is still no `block/buzz`
  remote; adding one back needs a *different* name now, since `upstream` is
  taken.

  For two days (2026-08-22 to 08-24) `origin` was the GitHub repo and there was
  no `upstream` at all — so the ceremony bullets further down this section,
  which say "`origin` = relay, `upstream` = GitHub", went from stale back to
  accurate without anyone editing them. Treat every remote name in this file as
  dated rather than current.

- **Nothing may hard-code a remote name, and nothing in a hook may prompt.**
  The 2026-08-24 repoint broke two pre-push guards, both silently and in
  different ways. `check-file-sizes-core.mjs` resolved the ratchet base from
  `origin/main`, which stopped resolving — every branch failed the gate at
  once, and since pre-push runs it, every push was blocked.
  `check-branch-skew.sh` ran `git fetch origin main`, which now meant the
  relay, which wants credentials a hook cannot supply: it hung two pushes for
  17 minutes each with **no output**, looking exactly like a slow build. Its
  `|| true` caught a fetch that *fails*, not one that never returns. Both now
  resolve from `main@{upstream}` → `origin/main` → `upstream/main`, and
  network-touching git in a hook sets `GIT_TERMINAL_PROMPT=0`. Diagnose this
  class by walking the process tree (`pgrep -P`), not by waiting.
- **The 15 retired branches are `archive/*` tags, not lost.** The ceremony
  branches (`integrated`, `integrated-build`, `integration/glue*`) and the
  pre-rebrand `feature/*` lineage were deleted locally on 2026-08-22 after the
  checkout read as a clone of the vanilla fork — 11 of 19 branches tracked refs
  that were `gone`. Each was tagged `archive/<branch>` first, so the commits
  stay reachable; `git tag -l 'archive/*'` lists them. Local-only, never
  pushed. Survivors as of that date: `main`, `rebrand/beekeeper`,
  `vanilla-patch`. `rebrand/beekeeper` was deleted local-and-origin on
  2026-08-23 once fully merged into `main` (same SHA both sides, zero commits
  absent) — no archive tag, because `main` already reaches its commits. **The
  branch list today is `main` and `vanilla-patch`**, the latter checked out in
  the `-vanilla` sibling worktree.
- **One agent in a ceremony worktree at a time, and never stage a tree
  wholesale.** Both rules were bought with destroyed work on 2026-08-20 (§2
  item 30). Two pipelines pointed at the same worktree interfered; the damage
  was done by `git checkout <rev> -- .`, which writes the index and working
  copy for every path at once and so silently overwrote an uncommitted edit.
  Inspect other revisions with `git show <rev>:<path>`, `git cat-file` or
  `git diff`. Fold with per-file
  `git diff <base> <head> -- <path> | git apply --index` (add `--3way` when
  context has drifted), and verify each folded file's changed-line multiset
  against its source diff afterwards.
- **An auto-staged rerere resolution is an unreviewed merge.** "Staged
  `<path>` using previous resolution" means Git wrote a merge result nobody
  looked at. Twice now the banked `state.rs` resolution has produced a file
  that does not compile (§2 item 30a, and `97dda378f` before it). After every
  assembly, check each auto-staged path by asserting that every line either
  parent added since the merge base survives in the result — and when one is
  wrong, fix the `rr-cache` postimage too, not just the working tree, or the
  next rebuild reintroduces it.
- **`git apply --3way` writes conflict markers into the file *and* records a
  rerere preimage.** Those resolutions are branch-scoped — a per-file fold
  deliberately drops other features' hunks — so leaving them in `rr-cache`
  risks replaying a deletion into the cross-feature assembly merge. Quarantine
  the day's entries before rebuilding; `rr-cache` is shared across every
  worktree of this checkout.
- **Provider identity is per app-instance, and the instance slug comes from
  the git branch** (`scripts/instance-env.sh`). Switching the worktree's
  branch gives the app a different app-data dir, a different provider
  identity, and a different session set. Sessions created under one branch
  cannot be resumed or stopped from another (§2 item 7).
- **Provider private keys live in the OS keychain**; the provider record
  (`<app-data>/session-provider/coding-session-provider.json`) holds only
  `providerPubkey`. So `BUZZ_DESKTOP_NOKEYRING=1` on an instance whose
  identities were created in keyring mode fails with "no key in JSON or
  keyring" — the mode switch needs a migration that does not exist yet.
- **`just dev` and `just desktop-standalone` differ**: only the latter
  honored `BUZZ_DESKTOP_NOKEYRING` until 2026-08-18. `just dev` also starts
  local Postgres/Redis/relay; `desktop-standalone` starts none of it.
- **The per-instance identity trap is broader than a branch switch — *how*
  you launch is the other half of it.** `just dev` uses the default OS keyring
  service `buzz-desktop-dev`, while `just desktop-standalone` scopes the
  service to `buzz-desktop-dev.<branch-slug>` (`Justfile:609`) and unsets
  `BUZZ_SHARE_IDENTITY` (`Justfile:603`); `just dev` sets neither, so it falls
  back to the plain default. So launching the *same* checkout the other way
  presents an empty keyring, and the provider refuses to start a session
  ("has no private key available"). The branch slug (bullet above) is only
  half the rule. Observed 2026-08-20.
- **Non-interactive shells do not source `~/.zshrc`**, so anything an agent
  launches misses profile exports (this is how the mode mismatch happened).
- Ceremony: `LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main)
  scripts/integrate.sh`. Park other worktrees on detached HEAD first. Push
  **both** remotes (`origin` = relay, `upstream` = GitHub).
- **`git-credential-nostr` intermittently 401s on the second push request.**
  The first request of a push authenticates, the next one comes back HTTP 401
  and the push aborts; an immediate plain retry succeeds with no other change.
  Suspected NIP-98 replay protection or clock-window handling on the relay's
  git endpoint. Observed twice on 2026-08-19, during the `build/2026-08-19.3`
  and `build/2026-08-19.4` pushes. Retry once before investigating anything.
- **Cross-feature merge conflicts are already solved on the assembly.** When
  a rebuild re-raises them, take `upstream/integrated`'s version of the
  conflicted file rather than reconstructing the union by hand.
- **A bad conflict resolution is persistent.** rerere records whatever you
  commit — including a broken union — and replays it into every later
  rebuild; the same unclosed `impl` came back three times on 2026-08-18. When
  a recurring cross-feature conflict reappears, take `upstream/integrated`'s
  version of the file. To purge a cached bad resolution:
  `grep -rl "<symbol from the hunk>" .git/rr-cache/*/preimage` and delete that
  directory.
- **Cargo.lock drift is invisible to tests and fatal to the deploy.** `cargo
  test` updates the lock in place, so a drifted lock passes every check and
  then fails the release image build. Run `cargo metadata --locked`; the
  ceremony gate and CI now do.
- **Work can be lost between ceremonies.** Two ledger commits made on a wip
  branch did not survive the next rebuild. After any ceremony, check that your
  own commits are present by content, not by assuming the rebase carried them.
- CI status without a login: `https://ci.agiterra.org/api/badges/1/cc.xml`
  (build number + status) — Brian has a Woodpecker login for the rest.
- Relay capability probes live in `/tmp/grant-proof` (`kindprobe`,
  `typeprobe`, `turn`, `stopcmd`): non-mutating checks for whether a
  deployed relay knows a kind or transition type.
- **Which build a relay is running can be read off a rejection message.**
  Run a member-key `bee pulse update` against a deliberately bogus project
  coordinate: a relay carrying the Pulse code rejects it `restricted: unknown
  project coordinate`, while an older build rejects it `unknown event kind`.
  The write never lands, so nothing is stored — this is the cheapest way to
  confirm a deploy from outside, without a cluster login.
- **A raw NUL byte in a TypeScript source makes git classify the file as
  binary**, which hides it from every diff and every reviewer: `git diff`
  prints "Binary files differ", so a whole-file rewrite reviews as nothing at
  all. Hit **twice**, both times a NUL used as a composite-key delimiter —
  `desktop/src/features/project-pulse/ui/ProjectPulseView.tsx` during the
  Pulse UX pass, and
  `desktop/src/features/agent-progress/lib/agentProgressSources.ts` on
  `wip/agent-sidebar` (fixed by `9cf627a0a`). Use `"\u0000"` in source, or a
  printable delimiter. Worth a lint rule.
- **Clearing a desktop install means four locations, not one.** On macOS a
  Tauri app keeps `localStorage` in `~/Library/WebKit/<bundle-id>`, *not* in
  Application Support — so clearing Application Support alone leaves the
  community list (`buzz-communities`, `communityStorage.ts`) and every
  onboarding flag intact, and the app comes back up exactly as it was.
  `scripts/reset-desktop-dev-state.sh:37-42` already encodes the full list —
  Application Support, Caches, WebKit, Preferences plist — and is the canonical
  reference even when resetting a *prod* bundle the script itself does not
  target. Two more traps in the same job: `security delete-generic-password`
  removes one entry per call, so loop it; and a running app or a detached
  sidecar will recreate directories behind you (two `buzz-shell-host`
  processes, alive since 13 and 17 August, kept recreating
  `xyz.block.buzz.app/shell-sessions` because the old path is baked into their
  argv — they outlive the app by design).
- **A quiet tick must still say something.** Both deployers now log one line
  per run — `repo=1 branch=main selected=<sha>(success) deployed=<sha> — up to
  date`. The `repo=` field is the assertion that a unit resolved its own
  config; it is the only practical runtime check, because
  `systemctl show -p Environment` returns **empty** for anything supplied via
  `EnvironmentFile` (that property covers only `Environment=` directives), so
  it reads as a missing config when nothing is wrong. Silence used to be the
  convention here, and silence is exactly what a deployer broken into a
  permanent no-op produces.
- **Both deployers are now one script in the repo**, `deploy/autodeploy/`,
  parameterized by `/etc/default/<unit>` via `EnvironmentFile=`. They had been
  two near-identical files, one of them untracked and root-owned, and every
  hazard below was found by reading the tracked one while still existing in the
  copy nobody could see. `just autodeploy-test` covers them; the cases were
  each verified to fail when the property is removed, because a test that
  cannot fail reads as coverage without being any.
- **`buzz-autodeploy` picks a pipeline by branch alone — no `repo_id`.** Its
  query is `where branch = 'integrated' and event = 'push' order by id desc
  limit 1`. Woodpecker now serves **two** repos (1 = `agiterra/buzz`,
  2 = `agiterra/beekeeper`), and beekeeper's branch is `main`. Today that is
  safe only because the watched branch names differ. The moment a deployer
  watches `main` — which hive's must — it will match the vanilla repo's `main`
  pipelines too and can deploy **stock upstream Buzz onto a Bee Keeper relay**,
  or the reverse. Pin `repo_id` in both deployers before wiring hive up.
- **A freshly-added Woodpecker repo has every Trusted flag off**, including
  Volumes. The gate mounts host caches from `/srv/ci-cache`, so without it the
  first run fails on the volume config and reads like a broken pipeline rather
  than a missing admin toggle. Only a server admin can set it.
- **Upstream `block/buzz` ships a failing test that this fork fixes.**
  `git-sign-nostr`'s `test_parse_envelope_rejects_invalid_oa_pubkey` asserts
  `parse_envelope` rejects an `oa[0]` that is hex-shaped but not a curve point;
  on the pristine tree it fails (`assertion failed: result.is_err()`), because
  nostr's `PublicKey` parses lazily and never checks. Our `b4e019bd9` adds the
  eight-line `XOnlyPublicKey::from_slice` guard. Verified both ways on
  2026-08-22: red on vanilla in Woodpecker #112, green on beekeeper `main`.
  This is the best upstream PR candidate we have — one self-contained commit
  that turns a test they already wrote green. **`agiterra/buzz` now carries it
  as a second patch** (`12201c49b`) on top of the CI one, because
  `buzz-autodeploy` only ships green pipelines and the vanilla gate could not
  go green without it. Revert that commit the moment upstream takes the fix.
- **Deleting a branch does not disarm `buzz-autodeploy`.** This was written
  here on 2026-08-22 as "autodeploy is inert because `integrated` no longer
  exists," and that was **wrong** — it selects from Woodpecker's `pipelines`
  table, which still holds every historical `integrated` row. It had been
  quietly resolving to fork build `393276ac0` the whole time, and staying
  silent only because that matched the deployed image. The instant lightyear's
  `BUZZ_IMAGE` changed to the vanilla tag, it woke up and tried to deploy **the
  fork image onto the freshly wiped vanilla relay** (journal, 03:17 UTC). The
  health check failed, it rolled back correctly, and the schema was still at
  32/32 afterwards — the fork build never migrated. Now pinned:
  `where repo_id = 1 and branch = 'main'`. The general rule: a deployer that
  reads CI history is armed as long as the history exists, whatever happened to
  the branch.
- **The pin was proven necessary the same minute it was applied.** Run
  side by side, the pinned query returned repo 1 / `12201c49b` (vanilla, equal
  to the deployed image → no-op) while the unpinned one returned repo **2** /
  `a0860552c` — a Bee Keeper commit, mid-build. Had it gone green first, an
  unpinned deployer would have put Bee Keeper on the vanilla relay, and it
  would have come up *healthy* while serving the wrong product.
- **Retired secrets accumulate in `/opt/buzz/compose/`.** Every autodeploy run
  leaves a `.env.bak-pre-<sha>`, each a full copy of the relay private key.
  33 of them had built up by 2026-08-22, plus a suffix-less `.env.bak` holding
  the key that had just been rotated out. All were deleted along with the 38
  pre-deploy SQL dumps when lightyear was wiped. If a deployer is ever
  re-enabled, give it a retention limit — the backups grow without bound and
  each one is a credential.
- **Verify desktop UI claims with the e2e mock bridge, not a release build.**
  The 2026-08-23 resume fix was proven by a 20-minute `just prod-desktop`
  build when `coding-sessions.spec.ts` already had the seeding scaffolding to
  prove it in seconds (`__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__`, a `TARGET` with a
  `generation` field). Two traps on that path: `pnpm build:e2e` runs `tsc`
  first, so an unused import fails the build and Playwright then serves the
  **previous** `dist` — a spec can pass against code that is not there; and
  `reuseExistingServer` keeps a stale preview on 4173. Always kill 4173 and
  check the build actually printed `built in`. The `just desktop-standalone`
  failure that pushed the work onto the release build was itself a bug
  (`fix/standalone-keyring-service`), not a workflow gap.
- **The relay's git hosting has no anonymous clone and no password auth.**
  Every request — clone included — needs a NIP-98 signed event, and the pushing
  key must be a relay member (`BUZZ_REQUIRE_RELAY_MEMBERSHIP=true` on hive).
  There is no read-only deploy token. Anything automated that reads the repo
  from the relay needs its own key with a roster or channel grant, not a URL.
- **The release and dev desktop builds hold *different* identities.** Debug
  builds use keyring service `beekeeper-desktop-dev`, release uses
  `beekeeper-desktop` (`desktop/src-tauri/src/app_state_keyring.rs:9-18`). The
  `agiterra-beekeeper` repo's kind:30617 author — the pubkey in the `origin`
  URL, `6cbdf445…92b68df2` — is the **release** identity. The dev key added to
  the relay on 2026-08-23, `npub1zxrgz5…` = `11868153…`, is not the repo owner
  and has no push grant. Pushing from a terminal means the release key.
- **The relay already held most of this repo's history, so the first push was
  1 MiB, not 534.** `size-pack` is 533.76 MiB and `BUZZ_GIT_MAX_PACK_BYTES`
  defaults to 500 MB (a request body limit), which looked like a blocker until
  `git ls-remote` showed the relay already carrying `ci/docs-only-path-filter`
  at `9da3042d1` — pushed by the desktop import, which pushes whatever branch
  is checked out. `main` was a 468-object delta from the merge base. **If a
  cold push is ever needed** (a new repo, a fresh relay), stage waypoints —
  `git push origin <sha>:refs/heads/main`, oldest first — so each pack is a
  delta, or raise the caps in a deploy window. `BUZZ_GIT_MAX_REPO_BYTES`
  defaults to 1 GB, which this repo is over halfway through.
- **`main` landed on the relay on 2026-08-24** at `c2a341acf`, the orphan
  `ci/docs-only-path-filter` was deleted, and **HEAD followed to `main` on its
  own** — the relay derives HEAD from the manifest, there is no default-branch
  setting to change.
- **A scoped `credential.<url>.helper` is appended to the helper list, not
  substituted for it.** `/opt/homebrew/etc/gitconfig` sets
  `credential.helper = osxkeychain` system-wide, so relay requests ran both
  helpers and every *successful* one printed `fatal: failed to store: -1` when
  osxkeychain tried to store an ephemeral credential. Write the helper as a
  two-value list with an empty reset first (`--unset-all` then `--add` per
  value; plain `git config key value` errors on a multi-valued key).
- **A push to the relay fails deterministically if anything slow sits between
  the ref advertisement and the pack upload.** Git mints its NIP-98 token at
  `GET info/refs`, then runs `pre-push` and builds the pack, then sends
  `POST git-receive-pack` with that same header. Under the ±60 s window a 99.9 s
  pre-push hook (desktop-test alone is ~100 s) made every push die with
  `RPC failed; HTTP 401`; `--no-verify` succeeded instantly, elapsed time being
  the only variable. Fixed 2026-08-24 by `BUZZ_GIT_NIP98_TOLERANCE_SECS`
  (default 600 s) on the git routes only — but note this **widens the read→write
  replay window** documented in `docs/git-nip98-method-binding.md`, because that
  transport does not bind the HTTP method. A large pack that takes over a minute
  to build would have hit the same wall with no hook involved.
- **`Keys::parse` accepts any 64 hex characters as a secret key**, so pasting a
  *public* key hex into `~/.nostr/key` yields a valid-looking, entirely
  different identity that every local check reports as fine — `6cbdf445…`
  pasted as a secret derives `508b1975…`. The bech32 `npub1…` form is rejected
  by name; the hex form is not decidable locally, which is why `bee git check`
  asks the relay instead. Hex and nsec are equally acceptable to the helper;
  nsec is preferable only because `npub`/`nsec` are visibly different.
- **The relay's push notification is a Nostr event, not a webhook.** Every
  ref-changing push publishes a relay-signed kind:30618 NIP-34 ref-state event
  (`crates/buzz-relay/src/api/git/manifest_event.rs:70-114`), carrying the refs
  and a `p` tag for the pusher. It is replaceable and is also emitted on repo
  creation, so a listener must compare refs rather than treat each as new work.
  There is no outbound HTTP webhook anywhere in the git path — `/hooks/{id}` is
  an *inbound* workflow trigger, and `buzz-workflow` has no git-push trigger.
- **Woodpecker's only GitHub coupling is its forge driver.** 3.17.0,
  `WOODPECKER_GITHUB=true`, one `forges` row, and every `repos`/`users`/`orgs`
  row carries `forge_id=1` — so a forge swap orphans every identity. Everything
  downstream is already forge-agnostic: `autodeploy` reads Woodpecker's sqlite
  and `git archive`s the local bare mirror. Pointing Woodpecker at the relay is
  not a settings change: it needs OAuth login, a repo/branch/file API, webhook
  delivery and commit statuses, and the relay has none of the four.
- **`nightly.yml` in this repo has never run.** The only Woodpecker cron row is
  `id=1, repo_id=1, branch=integrated` — the vanilla relay, on a dead branch.
  Nothing schedules the nightly for `repo_id=2`.

## 4. Authorities — unchanged, read when the question is "why"

| Document | Role |
| --- | --- |
| `SESSION_VISION.md` | Product authority: the ten invariants |
| `SESSION_DESIGN_PHASE_PLAN.md` | Design authority: D1–D6, gates G1–G7 |
| `SESSION_EXECUTION_PLAN.md` | Execution contract + rulings R1–R28 |
| `FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` | Continuity investigation, taxonomy, design space, addenda A–C |
| `P1_JUDGE_SCRIPT.md`, `P1_CONTINUITY_MATRIX.md` | The G1 experiment |
| `CODING_SESSION_UI_CONVERGENCE_HANDOFF.md` | UI convergence slices 2–4 (open) |
| `SESSION_PHASE_HANDOFF_2026-08-18.md` | Ship record + Andy's runbook |

`coding-session-analysis.md` is **not** history — it is the evergreen how-to
for querying stored session data (CLI + SQL). Updated 2026-08-19: its kind
table now covers 44220–44230 plus 44240 (Pulse), with per-kind fold rules and
query examples.

**Design studies landed 2026-08-20 night** (read-only recon and design work
from 2026-08-19/20, published so it is not redone — every claim in them
carries `file:line`):

| Document | The question it answers |
| --- | --- |
| `AGENT_PROGRESS_UI_DESIGN_NOTE.md` | What a live agent-progress surface looks like built buzz-natively — what t3code's Agents pipeline actually is (a pure client-side derivation over one WebSocket RPC stream: no polling, no file tailing) and what Buzz should build instead of copying it |
| `T3_AGENTS_SIDEBAR_CAPABILITY_INVENTORY.md` | The exhaustive capability list behind that note's §3 — every affordance of t3code's Agents sidebar, tab chrome to keyboard shortcuts, with citations |
| `AGENT_SIDEBAR_SURFACE_PICKER_DESIGN.md` | How a right-panel surface picker works in t3code (finding: **there is no surface registry**), what Buzz already has, and the Buzz design that follows |
| `T3_LAYOUT_WIDTH_STUDY.md` | Whether Buzz's 48rem transcript measure is wrong. Finding: **it is not** — t3code pins the same 48rem. The real delta is what happens to content that does not fit, which is what `b6032378` fixed |
| `T3_PROVIDER_NORMALIZATION_STUDY.md` | How t3code normalizes multiple agent providers — one schema-validated event union in a shared contracts package — and which part of that seam is worth taking |

History lives in `docs/archive/` (eight documents, moved 2026-08-18). Their
load-bearing content is extracted into §2 items 9–14 above; the rest is
provenance. Brian's working agreements moved from
`archive/SESSION_HANDOFF_SOL.md` §10 into `AGENTS.md`.

## 5. The rule that keeps this from becoming the nineteenth document

A finding from live use lands **here**, in §2, the day it is found — not in a
new document. New documents are for authorities and experiment protocols
only. When this file's §2 empties into shipped work, it shrinks; it is not an
append-only log.

**Note the irony deliberately:** this file is a hand-maintained ledger of
goals, decisions, open threads, and next steps — exactly the structure the
checkpoint design (report Addendum A) proposes storing as signed session
facts. Buzz should eventually hold this state itself, and this workflow is
its first customer.
