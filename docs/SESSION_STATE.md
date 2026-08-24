# Sessions — living state

**The one document to read first.** Everything else is either a binding
authority (§4), a protocol for a specific experiment, or history. This file
is updated on every build and whenever live use produces a finding; if it
disagrees with an older document about *current state*, this one wins.

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

_Last updated: 2026-08-23 night — a long live run on the dev instance against
hive produced six findings — §2 items 39-44, each with the log line or
`file:line` that proves it. Items 37 and 38, found earlier the same day, were
moved into the same section out of the "Recovered 2026-08-18" block, where
their numbers collided with items 18 and 19. All eight are the active track
(§3). `main` is at
`051f9771`; nothing from that run is fixed yet. Three environment facts from
the same run are in §3a: the dev instance must launch in keyring mode, a killed
`tauri dev` leaves vite holding its port, and prod and dev own different
providers._

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
| Unshipped locally | the `just dev` nokeyring fix, and three verified-missing session-stability fixes (§3) |
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
    loop (`acp.rs:1445`, dispatch `:1520`). No operator ever sees a prompt.
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

38. **The catalog trigger counts generations, not sessions.** A resumed
    session reads "Coding sessions (2)" and lists two entries that open the
    same umbrella. `desktop/tests/e2e/coding-sessions.spec.ts` ("a resumed
    session renders every earlier generation") asserts that real count on
    purpose; the fix must update the spec in the same change.

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
    - **What closes it, and it is the only thing that closes it.** Brian's
      instruction, 2026-08-23: *do not work around this — fix it.* So the
      deliverable is the MCP genuinely callable from a Codex execution, proven
      by a `tool_call: mcp__buzz-session-context__…` line in a provider log
      from a Codex execution against hive. Changing the continuity marker's
      wording is **not** a fix and does not close this item; it is only worth
      doing if the transport turns out to be impossible on codex-acp's side,
      and that finding would have to be recorded here with the evidence that
      established it.

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

44. **Coding-session lane messages also appear in the ordinary channel
    timeline.** `publishCodingSessionLaneRenderableRefs`
    (`desktop/src/features/messages/lib/codingSessionLaneVisibility.ts:104`)
    exists, is exported, and has no production caller, so the channel timeline
    never learns which `cs-session`-tagged kind:9 messages a lane has already
    claimed and renders them twice. The predicate it needs is already restated
    structurally at `:124` (`codingSessionLaneRenderableRefsFromUmbrellas`,
    matching `umbrellaHasCollapsedHistory`). Wire the publisher from the
    umbrella fold. Lowest impact of the seven.

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

**The active track, as of 2026-08-23 night, is the coding-session honesty pass
— §2 items 39-44, in that order (39, 40, 41 and 42 are what a user actually
hits; 43 and 44 are cheap).** It jumps this queue. The numbered list below is
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

- **This checkout has two remotes, and neither is `upstream`.** As of
  2026-08-22: `origin` = `agiterra/beekeeper` (the product), `vanilla` =
  `agiterra/buzz` (the block/buzz mirror plus the one CI patch). The
  `block/buzz` remote was removed — it was a second path to commits `vanilla`
  already carries, at a cost of 827 remote-tracking refs. Merge upstream with
  `git fetch vanilla && git merge vanilla/main`. **The ceremony bullets further
  down this section still say `upstream/main`, `upstream/integrated`, and
  "`origin` = relay, `upstream` = GitHub" — those remotes are gone.** The
  lessons in them (rerere, per-file folds) still hold; the remote names do not.
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
