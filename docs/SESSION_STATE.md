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
> relay at `lightyear.agiterra.org`; Beekeeper gets a new relay at
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

_Last updated: 2026-08-28 (later) — **the Dashboard**, built on
`worktree-dashboard` and not yet landed. Inbox, Pulse, Agent progress and
Agents are no longer four sidebar rows and four routes; they are tabs of one
Dashboard page at `/`, behind one sidebar row that wears the inbox badge.
**URL contract:** `/` is the Overview (no param) — one card per surface, each
a link to its tab, every number read through the same hook the tab uses
(`inboxBadgeCount` via `AppShellContext`, `useAgentProgress` with the
"at least N" floor preserved, `useManagedAgentsQuery` + `isManagedAgentActive`,
`useGlobalNotesQuery` ×3). `/?tab=inbox|pulse|agent-progress|agents` selects
a tab; `?item=` with no `tab` implies the inbox, so notification and
`beekeeper://` deep links still land on it. The old `/pulse`, `/agents`,
`/agent-progress` paths are `beforeLoad` redirects onto their tab (profile
panel keys carried across); `goAgents`/`goPulse`/`goProfile` target the tab
directly and there is a `goInbox` for channel-less notification clicks.
⌘⇧A, app launch and a community switch land on the Overview. Preview gates:
a `pulse`/`agent-progress` tab exists only when its flag is on; a URL naming
a hidden tab resolves to the Overview and fires the usual preview toast.
**One correctness point worth knowing:** the "viewing the inbox" flag that
marks the feed seen and zeroes the badge
(`useHomeFeedNotificationState`, `AppShell.tsx`) is now scoped to
`dashboardTab === "inbox"` — sitting on the Overview does not consume the
inbox. Only the selected tab body mounts (all four are lazy), so a hidden
tab never polls. Tab triggers carry the old sidebar testids
(`open-pulse-view`, `open-agent-progress-view`, `open-agents-view`, plus new
`open-inbox-view`/`open-overview-view`; the sidebar row is
`open-dashboard-view`). From a channel the strip is not on screen, so every
spec tab click is `openDashboardTab(page, tab)` (`tests/helpers/dashboard.ts`
— takes the Dashboard row first when needed); specs that landed on `/`
expecting the inbox now go to `/#/?tab=inbox`, and clicking "Inbox" in the
sidebar is `openInboxTab(page)`.
Gate: desktop unit 6521/6521, `tsc`, Biome and the px-text guard clean, new
`dashboard.spec.ts` 7/7; the 47 touched smoke specs 427/443, and every one
of the 16 failures **reproduces on a pristine `main` build** (control copy
via `git archive main`, same specs on port 4174): (a) **Settings → Agents
crashes** to the "Something went wrong!" boundary — `TypeError: Cannot read
properties of undefined (reading 'map')` in a `useState` initializer inside
the `AgentCreationPreview` chunk — which takes down every spec that opens
the AI-defaults settings (`agent-lifecycle-feedback` ×5,
`agent-numeric-tuning` ×3, `agent-provider-dropdowns` ×2,
`global-agent-config-screenshots` ×4); (b) `inbox-edit.spec.ts` "Edit and
Delete actions only for manageable messages" — a moderator Delete is
offered on another person's message; (c) `needs-restart-screenshots` 08
still expects "Buzz can restart it automatically" after `d62bcb029` renamed
it to Beekeeper. One more pre-existing break fixed in passing: `badge.spec.ts`
"primary navigation rows share the same inactive emphasis" asserted the 0.8
label opacity on the Projects row, which `0d7e001a1` moved out of
`.sidebar-primary-menu` where that rule lives (the spec now asserts the
Dashboard row only). **Not yet exercised against hive.**_

_Previously: 2026-08-28 — the project sidebar overhaul, built on
`worktree-project-sidebar-overhaul` and not yet landed. A project group is now
two flat lists (channels, then coding sessions + terminals) with no
collapsible sub-headers and no session cap; repositories, workflows, curated
agents and the Pulse row left the sidebar (all still on the project page —
Pulse as a route tab, `/projects/<id>?tab=pulse`, the old `/pulse` path
redirects). Under the session list a filter — **My sessions** (default,
persisted per project in `buzz.projects.sessionFilter.v1:<pubkey>:<relay>`),
**All sessions**, or **Custom** with member checkboxes (roster ∪ founders seen
in the list) — attributes by `founderPubkey`, the kind 44226 genesis signer;
a session with no resolved genesis cannot be attributed, so `mine`/`custom`
hide it and the trigger says `+N hidden`. Session rows wear the founder's
avatar; an unknown founder keeps the `Bot` glyph titled "Initiator unknown".
**Second pass, same day (Brian's follow-up):** the text status is a coloured
dot — blue Idle (every non-working provider word; the hover is just "Idle"),
green Running (last *reported* working; the hover keeps the not-a-live-lease
caveat), orange Closed, red Archived (`lib/projectSessionIndicator.ts`).
**`archived` is a new kind 44230 closure action** — core enum
`CodingSessionClosureAction::Archived`, relay authority founder-only like a
close, `open` reopens it; Pulse fold and the desktop coordination fold read it
as closed. **Older desktops reject an `archived` payload and therefore show
that session as open** (strict parser); acceptable for a single-team fork,
recorded here because it is a wire change. Row context menu: Close (open
only), Archive (not yet archived, open or closed), Reopen (settled). The
filter grew "Show closed" (default on) / "Show archived" (default off;
archived implies closed so it forces closed on) and a last-activity range —
Any time (default), Today, Yesterday, This week (Mon), This month, Custom
`from`/`to` date inputs; the trigger shows `+N hidden` across every axis. The
list pages ten rows at a time with a "Show more" row; the page resets when
the filter changes. Stored shape is now `{members, showClosed, showArchived,
range}`; the first release's bare `{mode}` blob upgrades in place.
Gate: desktop unit 6480/6480, `tsc` and Biome clean, the three touched smoke
specs (`projects-sidebar`, `projectPulse`, `agentProgress`) 19/19. One of
those, "entries lead while restarted generations remain disclosed", was
already failing on `main` (reproduced in a throwaway checkout of `main`):
it expected three *executions* for three resumed generations of one target,
but `sessionCoordinationFold` (`1bd574526`, 2026-08-21) counts an execution
by target identity and a resume extends it — the card honestly says "1
execution · 3 generations" and "Stopped" for the seeded older generations;
the spec now asserts that. **Not yet exercised against hive.**_

_Previously: 2026-08-26 — the mirror inversion landed and is live (banner
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
green at `62bcaa223`), hive serves Beekeeper, and **lightyear has been rebuilt
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
| Deployed (Beekeeper) | `beekeeper-relay:7d224a8b6` on **hive.agiterra.org**, observed running there 2026-08-23 (deploy time not recorded), with `a961fb277` pushed to `main` the same morning and awaiting the deployer's next tick. (Was `0be70d424`, shipped 2026-08-22 12:24 UTC by its own deployer (`deploy/autodeploy/`, installed as `beekeeper-autodeploy.timer`) — the first automated Beekeeper deploy. Its community row was created by `ensure_configured_community` on first boot from `RELAY_URL`; owner `6cbdf445…92b68df2`, key imported into the desktop keychain and the server copy deleted. Relay identity (NIP-11 `self`) is **`1fb029d0…c09ab336`** — recorded here as the baseline, because `BUZZ_RELAY_PRIVATE_KEY` auto-generates when unset, and a relay that silently rotates its key on every restart evicts every client cache. Both it and `BUZZ_GIT_HOOK_HMAC_SECRET` are persisted at 64 chars; check `self` against this value after any deploy.) |
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
    never resolved until Beekeeper cancelled it.** Andy's shared session is
    channel `7df9fd91-0066-461c-bc6b-5f49c6bb9a16`, session
    `ecde2480-c334-491d-ad6f-c8685e22ee02`, generation 1, signed projection
    `4b6fd011e70b1323…c150f4312b9` (`claude-agent-acp`, title "Rebuild Bee
    Keeper"). The signed kind-44225 sequence separates two failures that look
    like one spinner:
    - Turn 1 launched `scripts/local-prod-build.sh HEAD` as a Claude Terminal
      background command. Its tool result explicitly said "You will be
      notified when it completes" (event seq 20), but the ACP turn then ended
      successfully after 41,422 ms (seq 27). That promise is not a Beekeeper
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
      934,726 ms**. At Beekeeper's configured 900s silence boundary the
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
      moves an unresolved ACP request farther away; Beekeeper's timeout and
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
      boundary is Beekeeper's: it appears only after a signed running turn
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
    **Proven live 2026-08-26 (Brian, dev instance on the S1 tree, Codex
    `gpt-5.6-sol[medium]` against hive, session "S1 receipts test" in
    channel `2a9c83e7-94c5-4503-bc4f-024f695f20d1`):** six turns, every
    `user_prompt` carries its `csc-…` commandId; the two identical "Say
    the word ping" prompts sent 2 s apart each got their own id, receipts,
    and echo and settled in order on screen; 13 kind-44224 receipts —
    `created` plus `turn_queued` (5 keys) → `turn_started` (6 keys,
    `turnId`) per turn, no drops, no refusals; a draft queued locally
    during a `sleep 40` turn showed as "Next: Say pong · Cancel" and
    published as its own turn on settle. Not exercised live: the create-
    embedded first turn (the desktop sends the first prompt as its own
    44220), `turn_dropped`, `turn_refused`.
    Residuals: `DeliverError::Gone` and the
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
    (`~/Library/Application Support/Beekeeper/node-tools/lib/node_modules/
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
      cancellation".** That timestamp is Beekeeper's own flush. The translator
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
      **Completed with the answer intact**, plus a status row saying Beekeeper
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

60. **The local redaction reveal never resolved on the dev build, and the
    vault was innocent.** Andy, live on `context-redaction-improvement`: "I
    was still locally seeing the redacted message instead of the unredacted
    content, which was often my own input." Forensics on the dev instance's
    own disk cleared the provider side completely — every `sha256:` digest in
    the session-provider outbox had a matching plaintext line in
    `session-provider/<pubkey>/redactions/<session>.jsonl` (checked
    2026-08-27, session `bec3a33e…`, four of four). The failure was the
    desktop's read chain: `useRedactionDictionary` marked a scope "asked" the
    moment its IPC lookup *fired*, and the answer's `setResolved` was guarded
    by that effect instance's cancellation flag. Any unmount between fire and
    land left the next mount early-returning on the asked-guard while the
    answer arrived to nobody — and `React.StrictMode` (`main.tsx:80`)
    manufactures exactly that mount → cleanup → mount sequence for **every**
    transcript on **every** dev-mode render, so on the dev build the reveal
    could essentially never work on first view. Prod is exposed too, just
    less often: a pop-out or channel switch mid-lookup pinned the pill the
    same way.
    - **Fixed** by making the lookup a shared per-scope promise that any
      number of subscribers attach to (`subscribeRedactionResolution`): only
      a scope that *settled* empty is remembered as asked, a rejection is
      retryable on the next mount, and a landed answer serves later mounts
      synchronously from the module cache. The mount-cancel-remount sequence
      is pinned by `useRedactionDictionary.test.mjs` without a renderer.
    - **Rebase note, same area:** main had independently grown a second
      renderer for the same `[elided private context: …]` marker
      (`remarkPrivateContextMarkers` + `MarkdownPrivateContext`, from the
      multi-agent flow work). After rebasing, whichever plugin ran first in
      the remark chain won and only one of them knew about the vault. The
      pill is the superset, so the chip was deleted. The merged redactor now
      threads **both** main's workspace-relativization and the branch's
      recording log through one worker: a path inside the checkout publishes
      as a readable repo-relative path (never vaulted, nothing to reveal), a
      host path outside it publishes as a pill and is recoverable locally,
      and a credential publishes as main's fixed-width mask and is never
      recorded anywhere.


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

60. **The relay as the mailbox for turn delivery (Slice 2), built but not
    clean.** On `crew/s2-s6`, code at `734bd639`, not landed, not gated. (The
    SHA this item used to name, `ecb16a52`, is on no branch — `git branch -a
    --contains ecb16a52` is empty — so nothing cited against it resolved.)
    On the wire, kind 44220 gains a closed `deliver` class: the field is
    optional, absent means `boundary`, and the whole set is
    `boundary` | `steer` | `interrupt` — a closed serde enum, so anything else
    is rejected at decode rather than in `validate()`
    (`crates/buzz-core/src/coding_session_command.rs:57-66`, `from_wire` at
    `:83-90`; the relay envelope pins both halves at
    `crates/buzz-relay/src/handlers/ingest.rs:7474-7484`),
    the provider consumes a boundary turn at *start* and replays unconsumed
    turns from a persisted per-channel watermark in `(created_at, id)` order
    behind a 1.5 s reorder window, and three receipt codes join the now-open
    code list: `NO_LIVE_EXECUTION`, `NO_TURN_IN_FLIGHT`,
    `QUEUE_FULL_TURN_KEPT` (any nonblank, control-free ≤64-byte code is legal
    — the desktop still bounds at 256; that gap is now written into NIP-CSL).
    **Scope was narrowed and the narrowing is ratified:** S2 delivers "a turn
    is never silently lost", not "every accepted turn eventually runs" — a
    turn reaching no live execution gets a terminal `turn_dropped` /
    `NO_LIVE_EXECUTION` because `session.resume` mints generation N+1
    (`resume_session`'s `record.generation.checked_add(1)`,
    `crates/buzz-session-provider/src/lib.rs:1712-1716`) and the fence refuses
    the replayed generation-N command, so any receipt promising a replay would
    have been a lie.
    Re-addressing an owed turn to the resumed generation is the **sender's**
    job and is now in Slice 4's scope (desktop resend from the receipt; `bee
    sessions send --readdress`).
    Evidence: no completed `just ci` at this head; per-crate green counts as
    in §3 above; note that `just ci` runs **no** buzz-session-provider,
    buzz-sdk or buzz-relay lib tests at all (`justfile:427-464`,
    `scripts/run-tests.sh:78-146`) — only `.woodpecker/gate.yml:138` does.
    Round-3 triage applied on `crew/lane-2F`: `mailbox-1` (the hold branch
    advanced the watermark past older held turns) and `evidence-3` (a failed
    held delivery dropped the untried remainder) are fixed, each pinned red
    first. Round-2 triage, `contract-1`, fixed on `crew/s2-s6@734bd639`: the
    `evidence-3` hand-back itself created a silent-loss path for
    interrupts. `on_turn` records custody in `in_flight` only on its
    `Ok(()) if is_turn` arm and `is_turn` is false for `TurnAction::Interrupt`,
    so a cancel whose `SessionHandle::deliver` had already succeeded and whose
    ledger append then failed went back into `replay.held` with no record
    anywhere — both ledgers roll their in-memory entry back on a failed append
    — and the next redelivery passed every `decide_turn` fence and cancelled an
    unrelated running turn with no receipt, no transcript item and no ledger
    entry. A sibling process-local queue, `delivered_cancels`, now covers the
    window between delivery and the durable answer, read by `decide_turn` as
    `Ignored::AlreadyAccepted`
    (`crates/buzz-session-provider/src/commands.rs:520`).
    **Round-3 `contract-1` / `evidence-13`, fixed on `crew/lane-2G`:** that
    fence is *silent*, and it was closing over cancels nothing had answered.
    The interrupt arm wrote its ledger entry first and enqueued the operator's
    receipt second, so a failed append returned in front of the receipt while
    the cancel had already reached the actor and ended the running turn — no
    receipt, no transcript item, no ledger entry — and the silent redelivery
    then advanced the channel watermark past it, so not even a restart replayed
    it: a direct contract D ("never silently lost") and contract E ("an
    interrupt is answered by `interrupt_delivered` or `turn_refused`")
    violation. The outbox is itself a durable, crash-safe publish queue, so the
    answer now goes in front of the fence — `enqueue_receipt`
    (`crates/buzz-session-provider/src/lib.rs:2177`), then the consumed/refused
    append (`:2185-2189`). Pinned red first by
    `a_delivered_cancel_is_answered_once_even_when_the_ledger_append_fails`
    (`crates/buzz-session-provider/src/lib.rs:9866`), 0 passed / 1 failed
    before the fix (`receipt_stages` was `[]` where `interrupt_delivered` was
    expected). The fence is bounded now, too — a `VecDeque` capped at
    `DELIVERED_CANCEL_FENCE_CAPACITY = 256`
    (`crates/buzz-session-provider/src/lib.rs:171`), oldest evicted first,
    because its only release sits behind that same fallible append. Residuals: native mid-turn steer is boundary-only in **this build's
    desktop** — `NATIVE_STEER_DELIVERABLE` is `false`
    (`crates/buzz-session-provider/src/session.rs:78`) and `threadSteer` is
    therefore false for every execution, so the desktop never sends
    `deliver: "steer"`. The degrade path is **not** unreachable, and an earlier
    version of this item said it was: the relay accepts `deliver:"steer"` from
    any signer (`crates/buzz-relay/src/handlers/ingest.rs:7474-7479`),
    `inject_native_steer` then returns `false` unconditionally
    (`crates/buzz-session-provider/src/lib.rs:2264-2276`), and the arm behind
    the delivery publishes `turn_degraded`/`STEER_UNSUPPORTED` beside
    `turn_queued` (`crates/buzz-session-provider/src/lib.rs:2116-2124`) — so a
    `bee`- or CLI-published steer to a live execution produces that receipt
    today and every consumer must decode it. What is missing is end-to-end
    evidence, not reachability: all steer evidence is hand-injected; two briefed acceptances were **not met**
    (the kill-test now asserts both turns are *answered*, not that both *run*,
    and the queued row does not survive an app restart — it is module-level
    client state); a legacy ungoverned record still lets any channel member
    publish `deliver:"interrupt"` (deferred to S3/D7); the late rate-gated
    resubscribe in `buzz-acp` reopens a channel with no reorder window; and an
    `interrupt_delivered` receipt is decoded and stored but no surface watches
    it.
    **Update 2026-08-26: gated clean on `crew/s2-s6@1a3ee24d`.** CHECKPOINT
    cherry-picked all 15 lane-2G commits onto `c-int` clean, ending at
    `5bb4dff7` as expected, and ran lib tests across six crates (475+412+128+
    957+302+283 passed, 0 failed), clippy, desktop typecheck + vitest
    (6241/72/0), Tauri tests (2681+0+7+3+0 passed), `pnpm check:px-text`, and
    a full `just test` (12/12 groups, 0 failed, 75s) — all clean.
    **Proven live 2026-08-26 22:03** on the dev instance against hive (Claude·sonnet,
    gen 4, channel `b4cf9739…`): `ping` sent while a 32 s turn ran was
    `turn_queued` at 22:03:41 and `turn_started` at 22:04:11 — the same second
    the running turn's result landed. The `deliver`-omission hotfix was
    accepted by the deployed (pre-S2) relay on the same wire. **22:08, same
    execution:** Interrupt during a running `cargo test` turn → `interrupt_delivered`
    for the interrupt command, the turn's `result` = cancelled, and the turn
    queued behind it (`pong`) `turn_started` in the same second. Not yet live:
    queued-row escalation past 3 min, dropped/refused paths.

64. **A tool row keeps reading `Running` after its turn is cancelled.** Seen
    2026-08-26 22:08 on the dev instance (S2 tree, Claude·sonnet gen 4): an
    Interrupt during a `cargo test` turn closed the turn honestly — the
    boundary reads *Worked for 12s · cancelled* and the wire carries
    `interrupt_delivered` + a `result` with subtype `cancelled` — but the
    `Run Terminal` tool row above it still shows the spinner and `Running`,
    because no `tool_result` ever arrives for a call the cancel aborted and
    the projection keeps a tool's last state. A cancelled turn should close its
    open tool calls as *aborted with the turn* (renderClass status, no
    payload), derived from the turn's `result.subtype == "cancelled"` /
    `interrupted` item, not from a receipt. Desktop-only (projection); the
    mobile and web observers copy the same projection and inherit the gap.

65. **Agent seats (Slice 3), built and ungated on `crew/s2-s6`@`dbf54a38`.**
    An execution can now be created with an `actor`/`role` pair: the desktop
    stages the seat's nsec in a host-local 0600 custody file, the provider
    consumes it once and applies exactly the seat's four `BUZZ_` variables
    after the env fence, and the 44223 metadata carries `agentRef`/`role`.
    **There is no gate**: no `just ci`, no `just test`, and no explicit
    `cargo test -p <crate> --lib` run at any S3 head — only the pre-push hook
    chain, which prints no counts and does not run the buzz-session-provider,
    buzz-sdk or buzz-relay lib tests. Nothing has run against a real relay, a
    real desktop, or a real seat nsec (every test nsec is a non-parsing
    placeholder, so `Keys::parse` has never executed on this path) and the
    plan's relay-backed e2e was not written. The single refuter was
    same-family (advisory, not the cross-family tier-2 pass §1 requires) and
    returned CONFIRMED with 4 blocking findings, all applied on the branch;
    the sharpest was a control that lied — every seated create classified as
    malformed, so `resolveCodingSessionUmbrellaComposerAuthority` returned
    `canPromptExecutions: true` for every viewer. The first checkpoint push
    was blocked by the branch-skew guard; the finalizer rebased 84 commits
    onto `main`@`932ddf89` with zero conflicts, the pre-push chain passed in
    193 s, and the branch is on the relay. Owed: a real gate on this base with
    per-crate `--lib` counts, and one seated create watched on the dev
    instance.
    **Lead gate 2026-08-26 23:05 on 298a69f6 green:** lib 3439 passed, 0 failed; clippy
    clean; desktop 6285/0; Tauri 2701 passed, 0 failed; px clean; `just test` 2150 passed, 0 failed.

66. **Agents talk (Slice 4), built and gated green on
    `crew/s2-s6`@`cbbdf7e2`.** `bee sessions send/create/inbox/status` exist,
    every seat gets a roster and an inbox in its context package, and a
    refused turn can be re-addressed to the execution that resumed. Gate
    green: per-crate `cargo test --lib` over 8 crates
    (846/523/426/133/127/958/303/327, 0 failed), clippy 0 warnings, desktop
    6294/0, Tauri 2691/0/18, px clean, `just test` 12/12 — but **no full `just
    ci`** and **no live provider**, so `status`'s `live`/`quiet
    <age>`/`released`, `openTurn`, `queuedTurns` and inbox `stage` are
    unit-only and the slice's own acceptance (`bee sessions status` reads
    `quiet 3m`) is not met. Four contract flags — `--reply-to`,
    `--readdress`'s wire field, `--role`, `--driver` — are **refused with an
    explanation rather than implemented**, because each needs a `buzz-core`
    payload plus relay-validator change. The seated briefing
    (`agent_fence.rs:136`) still names none of the new verbs. Same-family
    refuter, CONFIRMED, five findings all applied. Pushed to the relay.

67. **Role packs and crew launch (Slice 5), built and RED on
    `crew/s2-s6`@`6168f63e`, not pushed.** Six vendor-agnostic role packs,
    seat skill materialization into the seat's workdir, and a desktop Crew tab
    that launches N seats and refuses a verifier of the builder's family. The
    Tauri suite fails `a_shared_workdir_is_refused_rather_than_written_into`
    (`nest.rs:907`) because the test asserts against the operator's real
    `$HOME`, which holds a stray `~/.agents/skills/brief/SKILL.md` **this
    program wrote before the guard existed** — delete it, and give the guard
    an injectable roots seam. The same test failed the pre-push chain, so
    nothing was pushed. Everything else was green (lib 3674/0, desktop 6335/0,
    `just test` 0 failed). Five same-family CONFIRMED findings are recorded
    and **none applied**: the family check is decided on a model string
    nothing verifies the provider can run while the UI claims otherwise;
    `resolve_seat_pack` stages a pack that need not contain the persona,
    turning a create that used to work into `PROVIDER_UNAVAILABLE`; and a
    symlinked `SKILL.md` inside an in-pack directory is read and copied into a
    seat as instructions. No crew can be authored from the desktop UI at all
    yet, and nothing here has run against a relay.
    **Re-gated `crew/s2-s6`@`d7eca684`: gate green** (lib 3679/0 across 8
    crates, clippy clean, desktop `pnpm test` 6345/0, Tauri 2710/0, `just
    test` 12/12). The stray `~/.agents/skills/brief/SKILL.md` from the
    earlier fix lane was removed by the lead. A single same-family advisory
    refuter pass CONFIRMED two blocking findings still unfixed — the
    `"default"` adapter alias still passes the crew's model-family check
    (`codingSessionCrew.ts:344`), and `buzz-persona/src/skills.rs:170`'s
    skill-write path still has no canonicalize/symlink guard on the target —
    so Slice 5 stays **not clean**.

68. **Budgets and liveness (Slice 6), built and gated GREEN on
    `crew/s2-s6`@`83c386fc`, not pushed.** A per-umbrella turn budget
    (`BUZZ_CSP_TURN_BUDGET`, a Settings control, a `turnBudget` key on 44223,
    a `bee sessions status` line) that refuses further agent-originated turns
    with `BUDGET_EXHAUSTED` once a crew has spent its allowance, with the
    human founder exempt. Gate: `cargo test -p <crate> --lib` over 8 crates
    3708 passed / 0 failed — the run `just ci` does not make — plus clippy
    `-D warnings` clean, desktop `pnpm test` 6365/0 over 72 suites, Tauri
    2712/0/18 (+7 csp, +3 rodio), `pnpm typecheck` and `pnpm check:px-text`
    clean, `just test` 12/12 with the workspace-integration binary 8/8. No
    full `just ci`, and **nothing ran live**: no provider was started with a
    budget, no `turnBudget` was seen on a wire, the Settings control was never
    clicked, and the end-to-end test drives the spend through
    `handle_session_event(SessionEvent::TurnStarted)` rather than a real
    adapter. The same-family advisory refuter CONFIRMED one blocking finding;
    two were fixed here, each pinned red first — an agent seat holding
    `BUZZ_PRIVATE_KEY` could make itself the umbrella's founder and so exempt
    itself from the budget forever (`claim_umbrella_founder`,
    `state.rs:519`), and an out-of-range `BUZZ_CSP_TURN_BUDGET` made the
    desktop drop the entire 44223 with no error surfaced. Five are deferred
    and unfixed: a create with no `sessionRef` is outside the budget
    (contract-1b); a queued batch can overshoot the limit by up to
    `SESSION_QUEUE_DEPTH` = 8 because `used` is charged at TurnStarted
    (contract-2); the create-path refusal reports `INITIAL_TURN_FAILED` with
    `BUDGET_EXHAUSTED` only in its message (contract-3); counts are
    provider-local, so an umbrella split across two provider hosts is
    disclosed wrong (contract-5); and the strict-decode deploy order needs a
    relay-before-desktop note (contract-6). `thread.turn.interrupt` is
    deliberately **not** budgeted, so a spent crew can still cancel its own
    runaway turn. The branch could not be pushed: the pre-push branch-skew
    guard reports it behind `origin/main` on 24 files it also touches, and
    hermit `just` was missing from the hook subshell so six other hook steps
    exited 127 before branch-skew ran.

69. **Mobile coding-session observer (read-only)** (`crew/mobile-observer`,
   built on lanes M1-M3: domain decoders, Riverpod state, and two pages).
   Renders a channel's umbrella-session list and one session's transcript
   from the same signed 44220-44230/24223 wire contract the desktop reads,
   with no composer and no command surface. It mirrors the desktop's
   read-side rules from `docs/CREW_SESSIONS_PLAN.md` §3: D3 — a turn's
   `deliver` class and any steer-to-boundary degrade are shown as received,
   never inferred from prose; D4 — rows settle on `turn_started` by
   `commandId`, and generation status/lease are kept out of the transcript
   queue itself; D5 — every provider's turns are read through one uniform
   fold, untargeted coding-session events bucketed by kind rather than by
   an assumed shape; D6 — an unresolved-actor failure is disclosed, not
   swallowed (pinned by the authority-unverified test); D7 — founder
   identity for close/genesis comes only from a receipt-joined signed
   create, so a session reads closed, and names who closed it, only on the
   founder's own word; D8 — a resumed generation is scoped to its own
   session and never bleeds into a sibling's history; D9 — an unread lease
   reads "reachability unknown," never "nobody answering"; D10 — no
   memory/context surface is rendered, consistent with memory staying out
   of scope. Gate here: `dart format` exit 0, `flutter analyze` exit 0 (no
   issues reported), `flutter test` was still running at last check (~661
   passed so far, none failed), and the repo-wide file-size gate had not
   yet run. Refuter (same-family, advisory — not a gating verdict):
   "does the mobile coding-session observer disclose what it could not
   read?" = CONFIRMED; test honesty = CONFIRMED. Deferred, one clause
   each: `mobile-observer-1` — `turn_degraded`/`interrupt_delivered`
   receipt error shapes are not pinned
   (`coding_session_decoders.dart:556-560`) because those statuses are not
   yet in this base's producer contract (D3/D7's native-steer and
   interrupt-delivery paths land in later slices); they still decode as
   turn stages, which is the property this branch needs. Not yet run on a
   device or simulator. Refute round 2 (`604b1ee5`) = CONFIRMED, one
   blocking: a disputed commandId's forged receipts revoked a target bound
   by a *different* undisputed create, erasing any session any member aimed
   at — fixed in `0b2b836a` (`coding_session_trust.dart`, pinned first by a
   failing test at `coding_session_trust_test.dart:262`); the two counts
   gaps (metadata conflicts missing from `isClean`, target-less receipt
   refusals uncounted) fixed with it; `order-1` (arrival-order
   `commandIdByTarget`) deferred as unreachable, named in the doc comment.
   Deferred-items pass 2026-08-26: `order-1` now files a generation under the
   command its signed order names (`943dd41c`); D10 eviction, per-kind refusal
   grounds and prompt attachment counts are now disclosed on both pages.

70. **Web coding-session observer (read-only).** `web/` gained
    `/sessions/:channel/:sessionId`, rendering the umbrella/founder header,
    the D3-D10 status fold with lease semantics, and the projected
    transcript, mirroring the mobile front's binding contract (target key,
    decoders, trust, generation resolution) rather than reimplementing it.
    Gate is red on this branch: `pnpm typecheck`, `pnpm check` (biome, 111
    files), and pubkey-truncation are clean, but `pnpm test` is 136/146
    (10 fail — every new/updated test fails once its fix is reverted,
    confirming they test real behavior), `pnpm build` and
    `just file-size-check` pass. Refuters returned same-family
    CONFIRMED/NOT-REFUTED findings, advisory only. Net: not clean, and not
    yet opened against a live relay.
    Round 2 (2026-08-26): the four fix-now items landed (page re-read whole,
    six turn statuses, create/resume-only generations, per-bound metadata
    tests); `just web-test` gates 151 green tests in `check`, `ci`, and CI.
    Round 3 (2026-08-26): the nine deferred coverage findings are pinned at
    165 unit tests (+14) and 4 e2e specs, each shown red under a named mutation.

71. **A push to the relay fails with HTTP 401 after long pre-push hooks.**
    Reproduced 2026-08-26 22:40 on `crew/web-observer`: every hook green
    (rust-tests 381 s, desktop-tauri-checks 857 s), then `RPC failed; HTTP
    401` on the upload; `git ls-remote origin` succeeded immediately after.
    Git mints the NIP-98 credential (`git-credential-nostr`) during ref
    discovery, *before* the pre-push hooks run, so a hook window longer than
    the relay's NIP-98 timestamp tolerance leaves an expired `Authorization`
    on the send-pack POST. The same signature killed the 21:45 pause push
    (≈10 min of hooks, then 401). Any branch whose diff pulls in the Rust or
    Tauri globs will hit it. Fix options: have the credential helper mint per
    request (git asks again on a 401 only if the helper is configured with
    `useHttpPath` and the previous credential is rejected — verify), widen
    the relay's NIP-98 window for `/git` pushes, or run the gate before
    `git push` and push with hooks already satisfied. Until then a 401 after
    green hooks is retried once with `--no-verify` on the identical SHA —
    never on a SHA the hooks did not see.

72. **A `select!` in the mesh demo echo loop drops frames — and the same
    cancel-safety hazard sits in `tunnel/reliable.rs`.** Found 2026-08-26 while
    making two `buzz-relay` lib tests hermetic (branch
    `crew/relay-test-flakes@470e8562`). `mesh_boot::run_demo_echo` awaits
    `ReliableMeshStream::recv_validated` — which reads the whole frame off the
    QUIC stream and only then awaits the Redis fence check — inside a
    `tokio::select!` against a 100 ms drain tick. When the receive loses the
    race after the read, the future is dropped and the frame is gone; the join
    leg times out at 10 s and answers 504. Measured: 7/8 standalone failures;
    the same body with no tick, or with the first tick delayed, passes 8/8.
    The demo route is gated on `BUZZ_MESH_DEMO_ECHO`, so no shipped flow is
    affected today, but any later tick landing during a fence check loses a
    frame, and session consumers will use the same stream. **Fix owed in
    product code** (`mesh_boot.rs`: pin the receive future outside the loop, or
    `tunnel/reliable.rs`: buffer the frame before awaiting the fence), then
    un-ignore `demo_join_forwarded_arm_round_trips_echo`. Second finding, fixed:
    `telemetry::trace_context_lookup_does_not_enable_callsites` asserted with
    `tracing::enabled!`, whose cached callsite interest folds over every live
    dispatcher in the process, so any other test's subscriber flipped it; it now
    asks this subscriber's own dispatch (verified non-vacuous). `cargo test -p
    buzz-relay --lib` 957 passed / 0 failed / 54 ignored, three runs. (Drafted
    as 63 while the crew branches were in flight; renumbered on landing to
    follow the last item `main` carries.)
    **Fixed 2026-08-26 (`crew/relay-cancel-safety`, round 2):** round 1's
    buffer only covered the fence window — the refuter showed a cancelled
    `read_exact` still misframes the stream (`short frame body`) and a fence
    slower than the cancel period restarts forever; `ReliableMeshStream` now
    moves the receive half into a reader task that reads, decodes and fences
    each frame exactly once into a bounded channel, so `recv_validated` is
    cancel-safe at every await. Test un-ignored; `--lib` 961/0/53, three runs.

74. **Every observer refused a seated create as malformed.** Found 2026-08-27
    20:0x by Brian: on the first seated session (`comssv2`, actor `ede63017…`,
    role `lead`) "Add a provider to this session" stayed gated with *"This
    session's founding record hasn't been resolved yet … wait a moment"* —
    forever. The genesis (44226), the seated 44221 and its `created` 44224
    were all on the wire; the desktop's founder/genesis observer
    (`codingSessionCreateObservations.ts`) enforced the pre-S3 exact key set
    on the create action, so any create carrying `actor`/`role` classified
    *malformed*, produced no observation, and the umbrella read
    `genesisResolution: "unresolved"`. The same key set lived in three more
    parsers: the desktop coordination strict-json fold (pulse/progress), the
    web observer's `lifecycleCommand.ts`, and mobile's
    `coding_session_session_decoders.dart` — so seated sessions were also
    invisible to the pulse and to both observers. **Fixed
    (`fix/seated-session-followups@cbfe2bde`):** all four accept the seat
    pair under buzz-core's rule (both or neither; actor lowercase 64-hex;
    role `[a-z0-9-]{1,64}`) and still refuse half a seat or a bad slug;
    red-before-green on each (desktop 26/26, web 25/25, mobile 35/35).
    Hot-applied to the live dev checkout as `48610354`. Lesson: S3 added a
    wire field to the *builder* and the relay; the ledger should have
    listed every exact-key reader of 44221 as a lane.

75. **A seated execution was briefed with its role's name and nothing else.**
    Same evening, same sessions: `roletest` and `comssv2` both answered
    "what is your role?" with "seated as lead" and a generic Claude Code
    self-description. Two causes. (a) Fizz is a builtin persona; the desktop
    builds every managed-agent record with `persona_team_dir: None` and every
    team with `crew: None` (S5 ledger, still true), so `resolve_seat_pack`
    found no pack and the seat ran nameless; linking the record by hand to
    `personas/roles/lead` (persona `lead`) made the pack materialize
    (`comssv2/.agents/skills/{write-brief,triage-report}` on disk). (b) Even
    with a pack, `session_briefing` (`session.rs`) appended only
    `actor_seat_briefing` (pubkey/role/relay); the persona body was resolved
    solely for `materialize_seat_skills` and never sent. **Fixed for (b)
    (`fix/seated-session-followups`):** `materialize_seat_skills` returns a
    `SeatRoleBriefing` (display name, persona body, skill names) and the
    briefing carries it — pack named, body verbatim, skills listed under
    `.agents/skills/`; a seat without a pack still gets only its role's name.
    Pinned by `a_seated_session_is_briefed_with_its_role_pack_prompt_and_skills`
    (reads the recorded `session/new` systemPrompt) and
    `a_seat_without_a_pack_gets_no_role_paragraph`; provider lib green.
    **Open for (a):** no UI path installs a role-pack agent or a crew block;
    the one-session seat field accepts an agent with no pack silently (the
    Crew tab says "seated with no role skills", the one-session path says
    nothing) — next item. Also open: the **Add-provider dialog has no
    agent-seat field**, so an agent cannot be brought into an existing
    session from the UI; today seats only exist at founding or via a crew
    launch. That is the crew conversation's front door and should be next.

73. **The pending create screen blamed the provider for a request the relay
    never took.** Found 2026-08-27 18:4x by Brian, first live crew launch
    after `e7d0e48b` deployed: "roletest" (seat `ede63017…`, role `lead`)
    showed *"The session provider has not accepted this request after 30
    seconds. It may be offline or not a member of this channel."* The wire
    said otherwise: zero 44221 from his key in that channel, ever. The dev
    app's durable create record (localStorage
    `buzz.coding-session-create.v1:project:…:bee-keeper`) held the signed
    event with `publishState: "ambiguous"`, `createdAt` 2026-08-26 23:47:16 —
    the night before, when hive still ran the pre-crew relay whose
    `deny_unknown_fields` rejected `actor`/`role`. Reopening the dialog the
    next day rehydrated that record, the lifecycle resolution read "pending"
    (no 44223/44224 for the commandId — correctly, nobody had seen it), the
    stall clock fired off the day-old `createdAt`, and the copy accused the
    provider. Worse, the stall *disabled* "Retry this exact request" on the
    theory that "the relay already holds these bytes" — false for exactly
    this state, and retry was the one action that would have worked (the
    provider's command horizon is 24 h). Both halves of the seated path were
    proven live the same evening: hive accepted a seated 44221 at 18:54:30
    and the dev provider `1958c6c4…` answered in the same second with the
    designed `ACTOR_UNAVAILABLE` refusal (no seat staged for that commandId).
    **Fixed (`fix/pending-create-honesty`):** `newCodingSessionStatusMessage`,
    `canRetryNewCodingSessionCreate` and `pendingCodingSessionWorkspaceStatus`
    take the record's `publishState`; `ambiguous`/`prepared` now reads *"The
    relay never confirmed this signed request, so no session provider has
    seen it. Retry this exact request to send it again, or start fresh."*,
    keeps retry enabled through a stall, and shows *Status unknown* instead
    of *Working*/*Idle*. Red-before-green in `newCodingSessionModel.test.mjs`
    and `PendingCodingSessionScreen.test.mjs`. Not changed: the stall clock
    itself, and the relay/provider (both behaved). Workaround on the old
    build: *Start fresh*. Side note from the same investigation: `bee
    sessions status` shows executions but not their founder, so a probe I
    sent to "a 3-day-quiet session" landed in Andy's (`7464daa5…` is his
    machine); his host answered `turn_dropped / NO_LIVE_EXECUTION` as
    designed. Founder should be a column.

76. **Crew front door — designer seat, role packs → actors with home roles,
    crew authoring, add a seat to a running session, seat disclosures, `bee
    events query` + founder column.** Built by a three-lane crew off
    `crew/front-door` (design `crew/lane-design`@`ecfcd3d1`, integration
    `fd-int`@`96aabfc7`).
    **Lane A — role packs become actors, crews can be authored**
    (`15db5efb`): `NewTeamCard` gains `Install crew roles…`
    (`data-testid="install-crew-roles"`) between Create team and Import;
    `InstallCrewRolesDialog.tsx` walks idle → chosen → installing → done
    (per-role result rows, skipped-child reasons, roster note) or
    nothing-found / failed; success toast `Installed {n} crew roles into
    "{team}": {roles}.`; team card badge `Crew · {n} seats`
    (`data-testid="team-crew-badge"`); agent row badges `Home role: {Role}`
    and `No role pack on this computer`
    (`data-testids agent-home-role` / `agent-no-role-pack`); a team-snapshot
    import preview discloses when a snapshot's crew could not be matched to
    its members. No mobile/web/CLI surface (design decision, needs sign-off).
    **Lane B — seats in the session UI** (`3d9a62e4`): the join dialog
    (`AddCodingSessionProviderDialog.tsx`) gets an agent-seat dropdown
    defaulted from the agent's home role, a role-mismatch notice, a no-pack
    notice, and blocks submit while half-filled; after submit, a
    `Seated: {Agent} · {Role}` line reports what custody actually staged
    (`data-testid="add-coding-session-provider-seat"`, addition beyond the
    brief); the pending screen
    (`data-testid="pending-coding-session-seat"`) and the session header
    (`data-testid="coding-session-header-seat"`) both show the seat. No
    mobile/web/CLI surface (design decision, needs sign-off).
    **Lane C — `bee events query` + founder column** (`602cccc0`): new
    subcommand `bee events query --kinds <n>[,<n>…] [--channel <uuid> | --h
    <v>] [--authors] [--ids] [--since] [--until] [--limit]`, raw signed
    events JSON newest-first, `--format compact` gives
    `{id, kind, pubkey, createdAt, h, summary}`; `bee sessions status` and
    `bee sessions list` gain `founder` + `createSigner` per execution and a
    channel-level `founders` array (JSON), `founder` in compact. Founder is
    keyed by execution identity (driver/instanceId/sessionId), not
    generation, so it survives resumes — deliberate deviation from the
    brief's literal wording, because keying by generation would print
    `founder: null` on every resumed session (the common case, and the
    subject of item 73 above). No desktop/mobile/web surface (design
    decision, needs sign-off).
    **Gate (`gatedSha` `9345d893`): GREEN.** `cargo test -p
    buzz-cli/buzz-session-provider/buzz-persona/buzz-core --lib`:
    573+441+157+381 = 1552 passed, 0 failed; `cargo clippy --workspace
    --all-targets -D warnings`: clean; `cargo fmt --all --check`: clean;
    desktop `pnpm typecheck` + `pnpm test`: tsc clean, 6448 tests 0 fail;
    `cargo test --manifest-path desktop/src-tauri/Cargo.toml`:
    2734+0+7+3+0 = 2744 passed, 0 failed, 18 ignored; desktop `pnpm
    check:px-text`: clean; `just check`: fmt/clippy/file-size/autodeploy/
    woodpecker all pass; `just web-test`: 166 tests, 0 fail. No failures.
    **Poke findings (verbatim), most severe first:**
    - *Blocking* — Agents view / agent profile panel — Home role and No role
      pack badges: "Nothing. Seven agents each carrying a home_role, one of
      them with has_role_pack:false, render as plain identity cards with no
      role and no warning — and the agent's own profile panel shows the
      same. The badges ManagedAgentRow.tsx:440-450 renders (`Home role:
      {Role}`, `No role pack on this computer`) appear on no screen."
      What is true: "ManagedAgentRow is only rendered by AgentGroupRows
      (desktop/src/features/agents/ui/AgentGroupRows.tsx:37), and nothing in
      desktop/src imports AgentGroupRows — grep across src/ returns only its
      own definition. The lane's stated disclosure (\"an agent that carries
      a home role but no pack must never render as if it carried the role's
      craft\") is implemented in dead UI." Screenshot:
      `desktop/test-results/fd1-poke/04-agents-no-home-role-badge.png`.
    - *Honesty* — Install crew roles dialog, roster note after a partial
      install: "\"Seated by default: lead, architect, builder, verifier,
      runner.\" — printed verbatim under a result list that contains no
      verifier row at all." What is true: "The verifier pack was not
      installed, so the crew that was written has four seats, not five.
      InstallCrewRolePacksResponse carries no field for a dropped roster
      role and installCrewRolesCopy.ts:20 is a constant string... Related:
      crewRoleResultRows computes a per-row `seated` flag
      (installCrewRolesCopy.ts:70) that InstallCrewRolesDialog.tsx never
      renders, so poker and designer — installed but deliberately unseated —
      read exactly like the five seated roles." Screenshot:
      `desktop/test-results/fd1-poke/15-install-dropped-role.png`.
    - *Honesty* — Install crew roles dialog, failure copy: "\"That folder
      could not be read: Error: the keychain is locked, so a new agent key
      could not be minted\"" What is true: "The folder was read fine; the
      install failed after the scan... everything reaching
      InstallCrewRolesDialog.tsx:84 unprefixed is something else —
      state.signing_keys() at :58, key minting, the managed-agent store, the
      team save — and the catch wraps all of it in the folder sentence
      anyway... An operator whose keychain is locked is sent to look at
      their folder." Screenshot:
      `desktop/test-results/fd1-poke/16-install-error-blames-folder.png`.
    - *Honesty* — Seat field (New session and Add-provider dialogs), no-pack
      disclosure: "\"Scribe has no role pack on this computer, so this seat
      carries no role skills and runs on its persona prompt alone.\" — for
      an ordinary managed agent that carries no role at all and was never
      asked about a pack." What is true: "codingSessionActorSeat.ts:180-190
      documents that an `undefined` hasRolePack must render nothing
      (\"absence is not a claim\")... But fromRawManagedAgent maps a missing
      has_role_pack to `false` (tauriManagedAgentRecord.ts:63), so
      `undefined` never reaches the component through the app's own data
      path: any backend that does not answer the field turns every agent
      into one whose pack is missing." Screenshot:
      `desktop/test-results/fd1-poke/11-seat-plain-agent-no-pack.png`.
    - *Note* — New coding session → Crew tab, seat roster before launch: "The
      tab itself says \"A crew launch is not resumable. If it stops
      partway, the seats already created stay\", so the operator learns a
      seat has no craft only once the seats exist. Every other pre-submit
      surface in this batch discloses it before signing; the crew roster
      does not." Screenshot:
      `desktop/test-results/fd1-poke/07-crew-roster.png`.
    - *Note* — Pending coding session screen, header status: pre-existing,
      outside this batch's diff — a green "WORKING" status sits two lines
      above the new seat line, which is itself correct. Screenshot:
      `desktop/test-results/fd1-poke/12-pending-seated.png`.
    (Poke spec: `desktop/tests/e2e/crew-front-door.spec.ts`, 864 lines, 8
    tests, committed `96aabfc7` on `crew/front-door` in `fd-int`, not
    pushed; 8/8 passed in 19.1s; 16 screenshots under
    `desktop/test-results/fd1-poke/`, all SHA-256-distinct.)
    **No-surface-by-decision (all need Brian's sign-off):** Lane A mobile —
    installing packs and minting keys is desktop-host-local (custody lives
    in the desktop keyring); the Flutter app is a read-only observer. Lane A
    web — same reason; the web client holds no keys. Lane A CLI — `bee`
    cannot reach the desktop's managed-agent store or keyring, and a second
    minting path is exactly the divergence that produces an agent the
    provider cannot resolve. Lane B mobile — the mobile observer is
    read-only and cannot create sessions at all. Lane B web — same. Lane B
    CLI — `bee sessions create --actor` is already refused on purpose
    (crew_cmds.rs:392-416): the CLI holds no host-local custody, so a seated
    create from `bee` would be answered ACTOR_UNAVAILABLE; lane C surfaces
    the seat on READ (status/list) instead. Lane C desktop — the desktop
    already shows founder provenance in the session header popover
    (founderDetails, CodingSessionUmbrellaWorkspace.tsx:366), and `bee
    events query` is a debugging verb for agents and the lead, not a
    screen. Lane C mobile/web — read-only observer, no CLI. Spec-wide — no
    crew editor this week (the installer writes one roster; editing seats by
    hand is deferred); no second builder seat in the default roster (the
    installer mints one agent per pack; cloning needs UI this batch didn't
    build); `home_role` is not editable in the agent dialog (it comes from
    the pack, and an editable copy could diverge from the pack actually
    staged); `home_role` is not published on kind:30175 (the wire already
    carries the seat's role, and a second differently-sourced role is a
    second answer to the same question).
    **Deviations/residuals (representative, see lane reports for the full
    lists):** desktop now has two agent-key-minting sites, not one — the
    shared `mint_agent_identity` helper (create + installer) and
    `confirm_team_snapshot_import`'s own inline mint block, left alone
    because it has an all-or-none rollback around minted pubkeys; the seated
    join-submit path could not be asserted through the DOM (no module
    mocking, submit gated on a Tauri-provisioned target), substituted with a
    live-mounted form plus a payload-builder unit test; live acceptance was
    NOT run for any lane — installing/seating/founder-probing against the
    running dev app was judged too risky from these worktrees (would mint
    keys and write records into Brian's live keyring/store, or requires a
    relay key this session was told not to go looking for); Lane A's
    `has_role_pack` resolution does per-agent disk IO on each 5s
    `list_managed_agents` poll for every agent that carries a pack link.
    **Next:** Brian's live look on the dev instance (installer walk, a
    seated join, `bee events query` / founder column against hive), then
    land. Findings F1–F5 from the adversarial pass are fixed on this branch
    (`4d84431f`, `ab3f96bc`, `e8c9eb03`, `e5e2511c`, with the front-door
    spec rewritten to prove them in `447ed22e`); F6 stays open.

77. **Live crew runs, 2026-08-27 evening — every open finding, queued for the
    next batch.** (Numbering: 74/75 live on `fix/seated-session-followups`,
    76 is reserved for the front-door batch's finalizer on `crew/front-door`;
    this item is written on the live checkout so nothing below depends on my
    context surviving.) Proven tonight, by hand-seated crews on Brian's dev
    provider `1958c6c4…`: lead→builder→lead over the relay with per-stage
    receipts (`comssv2`, channel `a02ce90b…`); a **Codex** builder under a
    **Claude** lead (`codextest`, channel `905116f0…`): brief → deviation →
    written amendment → report → operator BLOCK → Amendment 2 → rework →
    lead verified the live value → APPROVE (`codextest-builder@31f2ea99`);
    the S2 interrupt path on a seated agent (`interrupt_delivered` 22:12:30);
    the provider role briefing (ledger 75) live — the lead recites the five
    verbs. Two founder-column implementations exist and neither is landed:
    `comssv2-builder@50e8d18d` (Claude builder, bool column) and
    `codextest-builder@31f2ea99` (Codex builder, pubkey column, fails safe to
    null) — lane C of the front-door batch owns that surface and should take
    the Codex one. **Open, by owner:**
    - *Fence (S3, tier 2).* (a) Seats run with `HOME=/Users/brian` and share
      `~/.claude`, so Claude Code's local `SendMessage` and `Task` tools reach
      other sessions — the lead "dispatched" via a local subagent once and both
      seats talked outside the relay twice; give each seat its own
      `CLAUDE_CONFIG_DIR` (and the Codex equivalent) under the provider's state
      dir, and say in the seat briefing that the relay is the only channel to
      other seats and local subagent/cross-session tools are out of bounds.
      (b) Seats commit as the operator (`git` identity inherited); a seat
      should commit as itself. (c) `build_augmented_path`
      (`managed_agents/runtime/path.rs:104`) puts `~/.local/bin` before the
      app's own bundle dir, so a stale `bee` shadowed the shipped one — bundled
      binaries first. (d) A hand-seated identity needs an owner attestation
      (NIP-OA `auth` tag) or the relay's HTTP gate answers
      `relay_membership_required`; lane A's "install crew roles" must mint
      attested identities.
    - *Lead pack (`personas/roles/lead`).* (a) `triage-report`: verify the
      acceptance output yourself on the wire/binary before APPROVE — the lead
      approved a founder column that named the provider until the operator
      blocked it. (b) The crew's ledger is the relay: publish dispositions as
      Pulse entries (`bee pulse update`, kind 44240), not a repo doc the seat
      cannot edit. (c) After dispatching, end the turn; the report arrives as
      a turn — polling `bee sessions inbox` inside a 12-minute turn made the
      lead read every report twice and call it relay redelivery (the wire has
      one 44220 and one queued/started pair per command; six distinct turns).
      (d) `write-brief`: evidence = two or three `file:line` entry points, not
      an exploration (3½ min of grepping before the first brief); spell out the
      dispatch command with `--session-ref` (a role is only unique inside one
      umbrella; the CLI refuses `--to lead` without it).
    - *Seat briefing.* "Nothing wakes you between turns" is misleading for a
      seat — an addressed relay turn does; say so.
    - *Roster vocabulary.* `bee sessions grant --role` takes
      `collaborator|viewer` while seats carry crew roles (`lead`, `builder`);
      the same word means two things — designer decides the surface.
    - *Umbrella UI (designer + poker).* (a) "Last word" is the last execution
      with activity, so the lead's verdict collapses to a one-line pill while
      the builder's "Standing by" is expanded — the operator cannot see the
      crew is done; add a per-umbrella disposition strip (`lead · idle`,
      `B1 · builder · codex · APPROVE @ 31f2ea99 · not landed`) and expand the
      panel holding the latest *decision*. (b) Agent-to-agent turns render in
      full as the recipient's "user" bubble while the sender's panel collapses;
      the sender's panel is canonical, the recipient shows a reference.
      (c) The seat header shows the provider pubkey (`Builder 1958c6c4…`,
      identical on every row) instead of actor · role · vendor/model — an
      honesty bug; provider pubkey belongs in the provenance popover.
    - *Founder semantics (designer).* Every agent row reads "not founder"
      because only the human genesis signer is the founder; decide whether the
      operator question is "who founded" (session-level line) or "which seat
      holds authority" (the roster), then land one implementation.
    - *Roles are team artifacts; installs are per host (Brian, 23:1x).* The
      lane-A label "No role pack on this computer" reads as "roles are local";
      reword to "Role pack not installed here — install crew roles from the
      project's personas/roles". Overturn the designer's call to keep
      `home_role` off kind:30175: publish it as the agent's default role (the
      seat's 44223 role stays authoritative per execution; the UI discloses a
      mismatch) so any client sees "Fizz is a lead" without the pack
      installed. Packs should be fetched by project ref
      (`<repo>@<commit>:personas/roles/<role>`, the relay hosts git) rather
      than chosen by folder; the folder picker is this week's form. Agents
      stay per-owner (owner-attested keys; no shared custody) — same roles,
      same packs, different signers.
    - *Rulings 2026-08-28 morning (Brian).* (a) **"Crew" is renamed "Team"**
      everywhere a person reads it — the Crew tab, "Install crew roles…",
      "Crew · n seats", dialog copy; a team of agents is the thing you launch.
      Internal identifiers follow when touched. (b) *Pending turn:* S1's row
      stays as landed; carry Brian's forbidden-caption test ("thinking",
      "working", "generating" never appear in a live pending caption) as a
      small item; his silence-on-accept rule is parked until more dogfooding;
      `wip/pending-turn-brian` deleted (its other patches are on main via S1;
      ledger 51/52 are on main). (c) **F7, found on the review sheet:** the
      roster the installer writes is unlaunchable as installed — every seat
      reads "vendor not declared · sonnet" and the D8 family check refuses the
      launch with "change the seat in this computer's teams.json"; the
      installer must write the vendor it minted each seat on, and Claude
      aliases (`sonnet`, `opus`, `haiku`, `default` on claude-primary) must
      resolve to anthropic. Until then the front door opens onto a wall.
      (d) Refuting is a role (`verifier` pack), not a wrapper: `crew/tooling`
      deleted; the two builder-seat branches deleted (lane C's landed column
      supersedes them). Review sheet: `/Users/brian/Projects/beekeeper/review-2026-08-28/index.html`.
    - *Direction 2026-08-28 morning:* the team model — durable named
      identities, ephemeral seats, fixed home roles with versioned packs and
      project overlays, model + thinking per seat by rubric, the lead hires
      after hearing the mission, presets are playbooks, the Agents screen is
      the hub, desktop first — is written as `docs/CREW_SESSIONS_PLAN.md`
      §3.1 (D11–D16). It supersedes the folder-picker installer and the seat
      mismatch path from item 76; the next batch is briefed from it.
    Hand-edits on Brian's machine that these must replace: Fizz's record
    linked to `personas/roles/lead` by hand; `~/.local/bin/bee` repointed at
    `target/debug/bee`; `Application Support/Bee Keeper` copied to
    `…/Beekeeper`; provider binary swapped for the patched build from
    `seat-prompt` (a `tauri dev` rebuild will overwrite it).

78. **SWAT batch 2026-08-28 — the dogfooding position.** Three lanes on
    `crew/front-door` (`1b125465`, `7840dba8`, `2ca15bec`), briefed from item
    77's rulings and `docs/CREW_SESSIONS_PLAN.md` §3.1. The batch's purpose is
    narrow: make the installed team *launchable*, make a seat *behave like a
    seat*, and give the lead enough of this project to work from inside
    Beekeeper. Brian's standing go for the batch was "just send it home".

    **Lane R — the installed team launches, and it is called a team**
    (`1b125465`).
    - F7 part 1 (installer): `build_crew` now writes `driver=claude-agent-acp`
      and `vendor=anthropic` on every seat (`crew_roles.rs`
      `DEFAULT_CREW_SEAT_DRIVER` + `DRIVER_VENDORS` table, claude→anthropic,
      codex→openai, goose/buzz-agent deliberately absent because their
      provider is config-driven). Red test first:
      `every_seat_declares_the_runtime_and_vendor_it_will_launch_on` failed
      with "seat lead names no runtime" before the change.
    - F7 part 2 (desktop resolution): `resolveCodingSessionSeatVendor`
      consults the seat's ACP driver before the model id or the declaration.
      `CODING_SESSION_RUNTIME_VENDORS` maps driver slugs and
      `providerInstanceRef`s that run exactly one vendor; sonnet/opus/haiku/
      fable **and `default`** on `claude-agent-acp` all resolve anthropic,
      with a new `source:"runtime"`. A bare alias with no driver still
      resolves to nothing, and a declaration the runtime contradicts is a
      conflict, not an answer. The seat's driver is carried through
      `resolveCodingSessionCrewSeats`.
    - F7 part 3 (the launch actually enables): the default roster no longer
      seats a verifier — `CREW_SEAT_ROSTER` is `[lead, architect, builder,
      runner]`. Reason: `useCodingSessionCrewLaunch` publishes every seat's
      create against the one `providerInstanceRef` the dialog selected, so
      every seat runs on that runtime's vendor and a seated verifier can only
      ever share its builders'. D8 is kept intact and the roster is made
      launchable instead. The verifier pack still installs, unseated beside
      poker and designer, and the dialog copy states the reason. e2e test 03
      now drives the Team tab to an enabled Launch button with no refusal on
      screen.
    - New honesty guard: `checkCodingSessionCrewSeatModels` takes the
      provider's `instanceRef` and refuses when ANY seat's resolved vendor
      differs from the vendor that provider can run (e.g. the installed
      anthropic roster launched on `codex-primary`). Without it the written
      vendor would be an unchecked claim.
    - Rename (ruling 77a): "Crew" is "Team" in every user-facing string the
      lane owns — the New session tab label, "Install team roles…", the
      dialog title/body/roster plan, "installed, but not seated in the team",
      the success toast, "Team · n seats", "No teams with seats on this
      computer", "Launch team", "What is this team for?", the `[Team]` roster
      header in the lead's first turn, launch step "Check the team's model
      families", all launch failure copy, `CODING_SESSION_CREW_EDIT_HINT`,
      and `docs/CREW_ROLES.md` prose. Test ids and internal identifiers
      unchanged. The installer's team is now "Team roles", and a computer
      already holding "Crew roles" is renamed in place
      (`LEGACY_CREW_ROLES_TEAM_NAME` dedupe) rather than given a second team.
    - D11 named lead: the install dialog has a "Name the lead" field
      defaulting to "Lead", wired `installCrewRolePacks(directory, leadName)`
      → `install_crew_role_packs(leadName)` → `install_role_packs(lead_name)`.
      Red tests: the minted lead record carries the given name (Rust,
      `the_lead_is_minted_under_the_name_the_operator_gave` — "Keystone"), the
      builder keeps its role name, and the e2e spec types "Keystone" and
      asserts `leadName` reached the invoke.

    **Lane F — a seat cannot leave the relay, runs the shipped `bee`, and
    commits as itself** (`7840dba8`, item 77 *Fence*).
    - (1) **Research, no code — the negative result is the finding.** Measured
      on this machine 2026-08-28 against `claude` 2.1.248 and
      `@agentclientprotocol/claude-agent-acp` 0.70.0. **There is NO adapter arg
      or env var the provider can set that denies Task/Agent/SendMessage
      without breaking the seat's login.** Evidence: (a)
      `CLAUDE_CONFIG_DIR=/tmp/… claude -p …` answers "Not logged in · Please
      run /login" while the identical control run succeeds; seeding the
      relocated dir with `~/.claude.json` and pointing
      `CLAUDE_SECURESTORAGE_CONFIG_DIR` back at `~/.claude` both still fail —
      the macOS-keychain credential is keyed by the configuration home.
      (b) `CLAUDE_CODE_MANAGED_SETTINGS_PATH` exists as a string in the CLI
      binary, but a `permissions.deny:["Bash"]` file supplied that way had no
      effect (Bash ran); the same file passed as `--settings` removed Bash
      from the toolset outright — the file shape is right, the env var is not
      honoured. (c) The adapter's argv accepts only `--cli`, `--version`,
      `--hide-claude-auth` (`dist/index.js:10-40`, `acp-agent.js:277`).
      (d) The one working mechanism is per-session
      `_meta.claudeCode.options.disallowedTools`, merged by the adapter into
      the SDK query (`dist/acp-agent.d.ts:522`, `dist/acp-agent.js:4913`),
      built in `crates/buzz-acp/src/acp.rs` — outside the lane's ownership.
      **Codex:** no equivalent exists; `codex-acp` 1.6.2 exposes only
      `--client-name/--client-title/--client-version` on `login` and reads
      `CODEX_PATH`/`CODEX_CONFIG`, with no per-session tool denial and no
      subagent/cross-session tool of the kind the finding names.
    - (1b) Shipped instead, honestly labelled: `SEAT_OUT_OF_BOUNDS_TOOLS =
      ["Task","Agent","SendMessage"]` in `agent_fence.rs`, with the measured
      rationale in its doc comment under the heading "This list is a briefing,
      not an enforcement". It is live, not dead code — the seat briefing
      renders the names from it.
    - (2) Seat briefing (`agent_fence.rs` `actor_seat_briefing`): four new
      sentences — "The relay is the only channel to other seats and to the
      operator. Local subagent and cross-session tools — Task, Agent,
      SendMessage — are out of bounds in this seat…"; "After you dispatch work
      to another seat, end your turn. An addressed relay turn wakes you…";
      "Reports arrive as turns. Do not poll `bee sessions inbox` inside a
      turn…"; and the do-not-detach rule restated as "only an addressed relay
      turn wakes you, and a background job finishing is not one". The false
      "nothing reads this process between turns, so nothing will wake you" is
      gone from the **seated** briefing only (`FENCED_SESSION_BRIEFING`, the
      unseated one, is untouched). Four new tests, one per sentence, all four
      watched red by reverting the text (6 passed / 4 FAILED) and green after.
    - (3) `build_augmented_path` (`managed_agents/runtime/path.rs`): order is
      now exe-parent → managed npm bin → managed node bin → `~/.local/bin` →
      nvm → login-shell PATH → inherited PATH. This closes the "`~/.local/bin`
      shadows the bundled `bee`" trap §3 warns Andy about. New red test
      `bundled_binaries_outrank_local_bin` watched fail then pass; three
      existing order tests updated to the new contract.
    - (4) Git identity per seat: `ActorSeat::git_identity(role)` + four vars
      appended in `post_fence_env(role)` — `GIT_AUTHOR_NAME`/
      `GIT_COMMITTER_NAME` = display name, else role, else
      `agent-<pubkey16>`; `GIT_AUTHOR_EMAIL`/`GIT_COMMITTER_EMAIL` =
      `<pubkey16>@agents.beekeeper` (`SEAT_EMAIL_DOMAIN`, deliberately not a
      mailbox). Desktop `ActorSeatEntry` gains `displayName` (blank trims to
      absent). Red first: the new provider test failed to compile against the
      old 0-arg signature. End-to-end proof in `session.rs`: the seated
      subprocess-env test builds `post_fence_env` from a real `ActorSeat` and
      asserts the child's dumped environment contains all four values; the
      unseated twin asserts no variable contains `@agents.beekeeper`.

    **Lane L — the lead learns this week, and the umbrella shows team state**
    (`2ca15bec`).
    - Lead persona: dispatch **then end the turn** (the report arrives as a
      turn; no `bee sessions inbox` polling inside the dispatch turn); address
      seats with `bee sessions send --channel <uuid> --session-ref <uuid> --to
      <role>`, with the note that a role is unique only inside one umbrella
      (verified against `crates/buzz-cli/src/lib.rs:2395-2418`); the ledger is
      the relay — every disposition published as `bee pulse update --kind
      milestone --session <ref> --content "<lane> — <verdict> @ <sha> —
      <next>"` (kind 44240 = `KIND_PULSE_ENTRY`,
      `crates/buzz-core/src/kind.rs:619`); "no APPROVE on a report alone".
    - `skills/write-brief`: evidence = two or three `file:line` entry points,
      time-boxed to one read per file named, no grep sweep before the first
      brief; the template gains a `Seat:` line and ends with the exact
      dispatch command including `--session-ref`.
    - `skills/triage-report`: a **mandatory live-value check before APPROVE**
      (run the lane's acceptance yourself, read the value; "cannot run it
      here" is a BLOCK, not a note), with the founder-column incident as the
      reason; disposition published as a Pulse line.
    - New `personas/roles/lead/skills/choose-model/SKILL.md` (D13): task class
      → minimum tier (small/mid/frontier), modality → vendor (multimodal for
      images; a refuter never shares the builder's vendor), thinking level vs
      tier, and a mandatory "say why" clause. Written in tiers, never product
      slugs, so the pack stays model-agnostic.
    - New `personas/roles/lead/skills/beekeeper-project/SKILL.md` (114 lines):
      relay-canonical git (push `origin` only, the bridge mirrors GitHub,
      `rebase --signoff`, `commit -s`, never hard-code a remote), the item-71
      401-after-long-hooks retry rule, worktrees + the operator's hot live
      checkout + `activate-hermit`, the ledger discipline (SESSION_STATE is
      the record; in-flight dispositions go on the wire), the wire facts
      (44220–44230 named individually, per-stage receipts keyed by
      `commandId`, roles unique per umbrella), D11–D16, the quality gates, and
      what the operator rejects (the word "crew", menus, reports without
      `file:line`, comfortable guesses, approving on a report alone).
    - `plugin.json`: display name "Crew Lead" → "Team Lead", description and
      keywords de-crewed, version 0.1.0 → 0.2.0. The `id`
      (`com.beekeeper.crew.lead`) is an internal identifier and stayed.
    - Umbrella disposition strip: new pure selectors
      `listCodingSessionUmbrellaDispositions`, `codingSessionDispositionWord`
      and `formatCodingSessionDispositionLine` in
      `codingSessionUmbrellaModel.ts`, and `CodingSessionDispositionStrip`
      exported from `CodingSessionHeader.tsx`, rendered under the header for
      multi-execution umbrellas. One line per execution: `<agent> · <role> ·
      <live|idle|released> · last turn <age>`, the lead's execution first.
      **Honesty:** only working/idle/ended get the three team words; every
      other status keeps its own sentence, so an unreachable provider reads
      "no provider answering", never "idle". Status comes through the same
      reachability-demoted `deriveCodingSessionWorkspaceStatus` the focus
      chips use (resolver injected — reading the raw wire status would let a
      dead provider read `live`). An execution with no transcript renders "no
      turn observed" rather than an age it does not have. No new data fetch:
      every fact is 44223 metadata already on the catalog record. Red before
      green watched (missing-export `SyntaxError` on both new suites); 5 model
      tests + 2 header render tests added.

    **Gate on `crew/front-door` (green, run in `fd-int`):** `cargo test -p
    buzz-cli -p buzz-session-provider -p buzz-persona -p buzz-core --lib` →
    573 + 441 + 157 + 387 passed, 0 failed; `cargo clippy --workspace
    --all-targets -- -D warnings` → 0 warnings; `cargo fmt --all -- --check` →
    clean; desktop `pnpm typecheck && pnpm test` → clean, **6483 passed, 0
    failed**; `cargo test --manifest-path desktop/src-tauri/Cargo.toml` →
    **2744 passed, 0 failed, 18 ignored**; `pnpm check:px-text` → clean; `just
    file-size-check` → clean; `pnpm build:e2e && playwright test
    tests/e2e/crew-front-door.spec.ts --project=smoke` → **8 passed**. Logs
    `/tmp/swat1-gate-r1-{1..8}.log`. `just ci` itself was not run.

    **Deviations worth a ruling.**
    - *Product decision outside the brief's letter (lane R):* the default
      roster no longer seats a verifier. The brief asked for both "keep D8"
      and "a default roster that passes the family check"; those are only
      simultaneously satisfiable while every seat launches on one provider
      (`useCodingSessionCrewLaunch.ts:88-140`). Per-seat providers are the
      real fix and are a separate lane.
    - *Lane R could not implement "write the vendor the installer minted each
      seat on" literally:* the installer mints managed-agent records whose
      `agent_command` resolves to `buzz-agent` (`discovery.rs:290-295`; the
      packs declare no runtime), which is not the runtime a team seat runs on.
      Implemented instead as: the seat states the always-offered
      coding-session runtime, and a launch on a different provider is refused
      rather than silently run.
    - *Lane F stopped at the ownership boundary for enforcement.* The fix
      needs `crates/buzz-acp/src/acp.rs`: a setter beside
      `set_emit_raw_sdk_frames` (`acp.rs:1766`) and one line in the
      `session/new` `_meta` builder (`acp.rs:1405-1450`, beside
      `…["claudeCode"]["emitRawSDKMessages"]` at `acp.rs:1446`) writing
      `_meta.claudeCode.options.disallowedTools`; `CreateRequest` would carry
      the list. The lane deliberately did **not** add a dead `denied_tools`
      field, because a fence field nothing reads is the exact "control that
      lies about what it enforces" bug it exists to fix.
    - *Signature changes:* `post_fence_env()` gained `role: Option<&str>`
      (both call sites in `crates/buzz-session-provider/src/lib.rs` updated);
      `build_actor_seat_entry` gained `display_name: Option<&str>` (eight
      in-file test call sites). Two desktop PATH tests that asserted
      `~/.local/bin` is *first* now assert it is merely present.
    - *Lane L:* the umbrella workspace never passed a `seat` chip to
      `CodingSessionHeader`, so the strip is a new element rather than an
      extension of item 76's chip; and the derivation lives in the strip
      because `CodingSessionUmbrellaWorkspace.tsx` was 978/1000 lines and the
      memo pushed it to 1004 (`just file-size-check` failed; the limit was not
      raised).
    - *User-facing "crew" strings still owned by nobody in this batch:*
      `useCodingSessionCrewLaunch.ts:77,148,149`,
      `CodingSessionCapacityCard.tsx:382,407`,
      `codingSessionCapacity.ts:228` ("per crew session"),
      `CodingSessionsSettingsPanel.tsx:15`, and the `crewWarning` sentence
      rendered by `TeamSnapshotImportDialog.tsx:177` (text produced in Rust).
      Every other role pack's `plugin.json` still says "Crew <Role>".

    **Residuals.**
    - **No live acceptance anywhere in this batch.** Nothing was installed,
      minted, launched or committed against Brian's real keyring, provider or
      relay. All evidence is unit, Tauri-lib and mock-bridge e2e.
    - Item 77 *Fence* (a) **enforcement remains OPEN**: the fence for local
      subagent/cross-session tools is prompt-level only, and
      `SEAT_OUT_OF_BOUNDS_TOOLS`'s doc comment says so in as many words.
      Item 77 *Fence* (d) — owner attestation (NIP-OA `auth` tag) — untouched.
    - The provider-vendor refusal can only fire for runtimes that run exactly
      one vendor (claude, codex). A seat on `goose-primary` is unchecked,
      because goose takes its provider from `GOOSE_PROVIDER`.
    - The installed seats pin `driver=claude-agent-acp`. An operator who picks
      `codex-primary` or `goose-primary` gets a refusal naming the mismatch,
      and the only remedy this build offers is editing `teams.json` — there is
      no team editor (`CODING_SESSION_CREW_EDIT_HINT` says so).
    - The lead name sets the managed-agent record's `name` only; the
      `AgentDefinition`'s `display_name` stays the pack's ("Lead"), so a
      persona list shows "Lead" while the agent shows "Keystone".
    - `bee pulse update` needs a project coordinate (`--project` /
      `BUZZ_PULSE_PROJECT`, `crates/buzz-cli/src/lib.rs:2604-2626`). A team
      session with no project ref has no Pulse target and the lead persona
      does not say what to do then — **needs a ruling**: either the umbrella's
      `sessionRef` alone is a valid Pulse scope, or the lead needs a named
      fallback project.
    - The seat's git email uses a 16-hex pubkey prefix
      (`SEAT_EMAIL_PUBKEY_PREFIX`); the UI elsewhere abbreviates to 8. A Codex
      seat is briefed with Claude-Code tool names it does not have (harmless —
      it forbids what is absent — but a designer's eye is warranted).
    - A seat whose declared vendor agrees with its runtime prints just the
      vendor ("anthropic · sonnet"); the "(the only vendor claude-agent-acp
      runs)" annotation only appears for a seat that declares none. Both are
      honest; the two rows read differently for the same fact.
    - The strip's age is memoized on `[actorNames, resolveReachability,
      umbrella]`, so a quiet session's "last turn 4m ago" can lag.
    - `docs/CREW_ROLES.md` is renamed in prose only; the file name,
      `install_crew_role_packs`, `crew_roles.rs`, the `crew` block on the wire
      and every `data-testid` remain "crew" per the internal-identifier rule.
      Anyone reading the code still sees both words.

    **Follow-up `7ce3f74d` (2026-08-28, SWAT seat, on top of `e1dbc06a`
    "fence", parent `f4550c1d`).** Head `7ce3f74d` (strings) on top of
    `e1dbc06a` (fence), parent `f4550c1d`; `git pull --ff-only origin main` =
    Already up to date; tree clean before and after. **(1) The fence is now
    enforced on claude seats.** The exact `_meta` key path used is
    `_meta.claudeCode.options.disallowedTools` (a JSON array of tool-name
    strings), written on the session/new params object — confirmed by reading
    the managed runtime adapter at `~/Library/Application
    Support/Beekeeper/node-tools/lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js`:
    line 4766-4767 `const sessionMeta = params._meta; const
    userProvidedOptions = sessionMeta?.claudeCode?.options;` and line 4913
    `disallowedTools: [...(userProvidedOptions?.disallowedTools || []),
    ...disallowedTools],` inside `createSession`. Scope extension (deliberate,
    worth a ruling): the key is also written on `session/resume` and
    `session/load`, not only `session/new` — reason measured in the same file,
    `newSession` (4747), `resumeSession` (778) and `loadSession` (788) all
    funnel into `createSession`, and `getOrCreateSession` (4645-4667) forwards
    `_meta: params._meta` verbatim; our provider always spawns a fresh adapter
    process, so a reattached seat re-enters `createSession` and without
    restating the denial the fence would lapse at the first resume.
    Implemented as one shared helper `AcpClient::apply_disallowed_tools_meta`
    called from all three. In `buzz-acp`: new private field `disallowed_tools:
    Vec<String>` (`acp.rs` ~line 723, default empty at the constructor),
    public setter `set_disallowed_tools(&mut self, tools: &[&str])`
    immediately after `set_emit_raw_sdk_frames` (`acp.rs:1766` area), and the
    merge helper; an empty list omits the key entirely rather than sending
    `[]`, so a session that denies nothing is byte-identical to one from
    before the option existed, and the doc comments on the new public API
    state that the adapter reads it once, at the opening request, so a later
    call is inert. In `buzz-session-provider`: `start_agent` (`session.rs`,
    beside `client.set_emit_raw_sdk_frames`) calls
    `client.set_disallowed_tools(crate::agent_fence::SEAT_OUT_OF_BOUNDS_TOOLS)`
    gated on `request.seat.is_some()`; unseated executions are unchanged,
    proved by the green unseated test asserting `/params/_meta/claudeCode` is
    absent entirely. Red before green, evidence 1 of 2 — `buzz-acp`
    compile-red: `cargo test -p buzz-acp --lib denied_tools` → ``error[E0599]:
    no method named `set_disallowed_tools` found for struct
    `acp::AcpClient` `` at `acp.rs:5130` and `acp.rs:5157`, ``error: could not
    compile `buzz-acp` (lib test) due to 2 previous errors``; three tests
    added: `denied_tools_reach_session_new_meta`,
    `denied_tools_reach_session_resume_meta`,
    `a_session_that_denied_nothing_sends_no_disallowed_tools_key`. Evidence 2
    of 2 — `buzz-session-provider` assertion-red on the recorded wire: `test
    session::tests::a_seated_create_denies_the_out_of_bounds_tools_on_session_new
    ... FAILED` at `session.rs:2598`, `left: None / right: Some(Array
    [String("Task"), String("Agent"), String("SendMessage")])`, with the
    recorded `session/new` dumped in the failure showing
    `"_meta":{"sessionTitle":"Ship it"}` and no `claudeCode` key; its twin
    `an_unseated_create_denies_no_tools ... ok` passed both before and after,
    which is the point of the pair. Both green after the change; harness
    `MCP_RECORDING_AGENT` + `request_by_method`. `SEAT_OUT_OF_BOUNDS_TOOLS`'s
    doc comment (`agent_fence.rs`) was rewritten: the heading "This list is a
    briefing, not an enforcement" is gone and it now reads "Enforced on
    claude-agent-acp; a briefing on codex-acp", saying in as many words that
    `codex-acp` 1.6.2 exposes no per-session tool denial so there is nothing
    to enforce with there and `actor_seat_briefing` is the whole fence for a
    codex seat; lane F's two measured dead ends (relocated
    `CLAUDE_CONFIG_DIR` breaking the keychain-keyed login;
    `CLAUDE_CODE_MANAGED_SETTINGS_PATH` being unhonoured while `--settings`
    works) are kept verbatim under "Mechanisms measured and rejected" so
    nobody retries them. **Item 77 *Fence* (a) is now closed for claude seats
    and explicitly still open for codex seats** — the residual above and the
    "Lane F stopped at the ownership boundary" deviation are stale as of this
    commit. **(2) The last operator-facing "crew" strings say team.** All five
    briefed files done, copy only — identifiers, test ids and the
    `crewWarning` wire field untouched: `useCodingSessionCrewLaunch.ts` ("run
    this crew"→team, "Timed out while seating the crew."→team, "Failed to seat
    the crew."→team); `CodingSessionCapacityCard.tsx` (five strings — the
    "Turns per crew session" label, "Reading this computer's crew budget…",
    "per crew session.", "A crew session is one launch…", "Could not save the
    crew turn budget."); `codingSessionCapacity.ts:228` ("per crew session");
    `CodingSessionsSettingsPanel.tsx:15` ("how many turns a crew session may
    take"). The `crewWarning` producer was found and changed:
    `desktop/src-tauri/src/commands/team_snapshot/import_crew.rs`, const
    `CREW_UNMATCHED_NOTE` (the string `TeamSnapshotImportDialog.tsx:177`
    renders), new copy "This snapshot's seat roster could not be matched to
    its members, so it was imported as an ordinary team." — deliberately *not*
    a literal crew→team swap, which would give "This snapshot's team could not
    be matched to its members, so it was imported as an ordinary team" and
    name the wrong noun twice; what failed to bind is the seating plan inside
    the team, and the team itself imported fine. The Rust test asserting the
    old sentence (`team_snapshot/tests.rs:827`) was updated with it. Two
    extra edits beyond the brief's file list, both forced by honesty rather
    than tidiness: (a) the budget refusal message produced in Rust *quotes the
    settings control by name* — `crates/buzz-session-provider/src/commands.rs:624`
    and `lib.rs:1575` both said `raising "Turns per crew session" takes
    effect…`, and renaming the control without these would have sent the
    operator hunting for a control that no longer exists, so both were renamed
    in the same commit along with their two desktop test assertions
    (`codingSessionTurnRefusal.test.mjs:64,67`); (b)
    `CodingSessionComposerDeck.tsx` read the same number under `label="Crew
    turns"` with the sentence "The turn count is the whole crew session's…",
    renamed to "Team turns" / "team session's" so the Info popover and the
    settings panel agree. Remaining "crew" strings, reported not changed
    (outside this brief's scope): six role packs' `plugin.json` display names
    and keywords still say "Crew <Role>"
    (`personas/roles/{architect,builder,runner,verifier,poker,designer}/.plugin/plugin.json`
    lines 4 and 7 — only lead was renamed, by lane L); agent-facing prompt
    text `crates/buzz-session-provider/src/session.rs:1069`
    `FRESH_CREW_BOOTSTRAP_PREFIX` contains "this session is a crew room",
    which is read by the agent, not the operator, and names no UI control, so
    it was left — but it is the last non-identifier "crew" an agent will see;
    internal-only and correctly left alone are all `crew`/`Crew`/`CREW_*`
    identifiers, the `com.beekeeper.crew.*` plugin ids, the `crewWarning` wire
    field, `data-testid="team-snapshot-import-crew-warning"`, and "crew"
    inside doc comments (e.g. `codingSessionCapacity.ts:29-39`,
    `tauriSessionProvider.ts:91-142`). **Gate — every command from the brief,
    run in `fd-int` under hermit, exit lines observed:** `cargo test -p
    buzz-acp -p buzz-session-provider --lib` → 865 passed / 0 failed and 389
    passed / 0 failed, EXIT:0; `cargo clippy -p buzz-acp -p
    buzz-session-provider --all-targets -- -D warnings` → EXIT:0, zero
    warnings; `cargo fmt --all -- --check` → EXIT:0; desktop `pnpm typecheck`
    → EXIT:0 and `pnpm test` → 6483 tests, 6483 pass, 0 fail; `pnpm
    check:px-text` → EXIT:0; `cargo test --manifest-path
    desktop/src-tauri/Cargo.toml --lib -- snapshot crew` → 273 passed / 0
    failed (unfiltered suite also run: 2744 passed, 0 failed, 18 ignored);
    `just file-size-check` → EXIT:0; extra, since `import_crew.rs` changed,
    `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets
    -- -D warnings` → EXIT:0. **Not done / residual:** no live acceptance —
    nothing was launched against a real provider, so the claim that the
    adapter actually drops Task/Agent/SendMessage from a running seat's
    toolset rests on the adapter source at `acp-agent.js:4913` plus the
    recorded wire, not on a seat that tried to call Task and could not; that
    is the one check worth doing on the next real launch.

    **Next — Brian dogfoods from inside Beekeeper.** Relaunch the dev app on
    `main`; `Install team roles…` from `personas/roles`; name the lead
    **Keystone**; start a session with Keystone on **claude-fable-5**; seat
    builders via **Add provider**; then the lead works from inside Beekeeper.
    Everything above is unproven until that walk happens.

79. **First team launch from the UI (Brian, 2026-08-28 ~11:00) — what the
    front door got wrong on contact.** Install team roles worked (Keystone +
    Architect/Builder/Runner seated, Verifier/Poker/Designer unseated, `Team
    roles` carries driver+vendor per seat). Then: (a) **The Agents grid shows
    the persona card's name, not the identity's.** The installer named the
    identity `Keystone` but reused Fizz's persona card and titled it `Lead`
    (managed-agents.json: persona record `name: "Lead"`, agent record `name:
    "Keystone"`), so the grid read "Lead" with Fizz's avatar. Hand-fixed on
    Brian's machine (persona card renamed); the installer must name the
    persona card it creates or reuses after the identity, and the grid should
    show the identity's name. (b) **No filter on the Agents screen** — with
    every team member's agents on one relay Brian wants to see *his* agents
    (owner) and a team's agents; D16 hub item. (c) **A team launch is
    single-provider, and the refusal copy is inverted.** Brian set Architect's
    model to `gpt-5.6-sol`; the roster read *"declared openai, but gpt-5.6-sol
    is anthropic"* and Launch stayed disabled. The truth: the seat is pinned to
    `claude-agent-acp` (anthropic) by the installer and a team launch runs
    every seat on the one provider the dialog holds, so a Codex seat cannot be
    launched with the team today (lane R residual, item 78). The copy must say
    that — "this seat runs on Claude Code; gpt-5.6-sol is an OpenAI model" —
    and D13 needs **per-seat provider** in the team launch. Workaround: launch
    the team on Claude, then hire the Codex architect into the umbrella via
    Add provider → seat (that path picks the provider per seat). (d) The model
    change was made on the persona card and did not reach the agent record
    (`model: None`) — two places to set one model; the identity's model should
    be the one the roster reads. (e) "Every seat runs on Claude Code…" copy is
    honest about (c) but reads as a product rule; it is a limitation, say so.
    (f) Team picker auto-selects the only team — expected, not a bug. (g)
    The Team tab was disabled in a project context because the team launch
    never minted the sessions channel and canLaunch said nothing — fixed
    here.

    **Fixed e772dd74:** (1) Team launch from a project now mints the
    channel: launchCodingSessionCrew settles it before the genesis via the
    project flow's own ensureChannelId helper (no second implementation) —
    /Users/brian/Projects/beekeeper/beekeeper.worktrees/fd-int/desktop/src/features/coding-sessions/lib/codingSessionCrewLaunch.ts:173
    (new CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP) and :333
    (resolve-then-genesis, id threaded into every dep call); hook wiring at
    ui/useCodingSessionCrewLaunch.ts:94 (provider membership moved inside
    publishGenesis, since the channel may not exist until the launch
    publishes it); dialog passes projectContext.ensureChannelId at
    ui/NewCodingSessionDialog.tsx:554. RED first:
    lib/codingSessionCrewLaunch.test.mjs:358/409/432 failed with 'does not
    provide an export named CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP' before
    the change, then 19/19 pass — the ordering assertion is a literal
    deepEqual of
    ['channel','genesis:chan-minted','publish:lead:chan-minted','receipt:...','grant:...','turn:...'],
    and a known channelId path throws if ensureChannel is called.

    (2) A disabled launch names its reason: pure
    codingSessionCrewLaunchBlock in lib/codingSessionCrew.ts:372 covers all
    six conditions (launch in flight, in-flight create for this scope, no
    team, no seats, no channel and no way to make one, empty goal); the tab
    computes it at ui/NewCodingSessionCrewTab.tsx:150 and disables the
    button on exactly that expression (:159), rendering the sentence at
    data-testid new-coding-session-crew-blocked (:267) as a sibling of the
    existing -refusal element. RED first: lib/codingSessionCrew.test.mjs:584
    failed on the missing export; e2e now drives it at
    desktop/tests/e2e/crew-front-door.spec.ts:827 ('Write the goal — the
    lead's first turn carries it.' with Launch disabled) and :841 (gone once
    the goal is typed).

    (3) Persona card vs identity: the installer titles the card it creates
    or reuses after the identity it minted (lead name included) —
    desktop/src-tauri/src/managed_agents/crew_roles.rs:527 (display_name:
    agent_name.clone(), name computation moved above the definition); the
    grid card shows the instance's name when one backs it — new
    lib/agentCardTitle.ts (resolveAgentCardTitle) used at
    desktop/src/features/agents/ui/UnifiedAgentsSection.tsx:260. RED first:
    crew_roles_tests.rs:580 and :637 both failed 'left: "lead", right:
    "Keystone"'; agentCardTitle.test.mjs failed on the missing module. Two
    pre-existing tests in UnifiedAgentsSectionCardTarget.test.mjs queried
    the card by the persona name and were updated to the instance name (the
    click target they assert is unchanged).

    (4) Vendor copy: describeCodingSessionSeatVendor's conflict branch
    (lib/codingSessionCrew.ts:307) now distinguishes a runtime conflict from
    a seat's own two statements — the resolution carries `runtime`/`via` —
    so a Claude-pinned seat with gpt-5.6-sol reads 'runs on Claude Code
    (anthropic); gpt-5.6-sol is an OpenAI model — a team launch runs every
    seat on one provider' instead of the false 'declared openai, but
    gpt-5.6-sol is anthropic'; the family refusal lead-in stopped calling it
    a declaration-vs-model-id fight. Helper text now reads 'Today a team
    launch runs every seat on one provider — <runtime> — …  Seats on another
    provider are added to the session afterwards.'
    (ui/NewCodingSessionCrewTab.tsx:235), asserted in the e2e. RED first:
    codingSessionCrew.test.mjs:542 plus the runtime-conflict deepEqual and
    the new family-refusal test all failed before the change.

    Commits (oldest first): d7525d91 fix(team) channel minting + refusal
    sentences + honest vendor/limitation copy; 61648432 fix(agents) card
    titled after the identity; 46cc7784 test(e2e) Team tab assertions;
    e772dd74 docs(team) hook doc. Working tree clean; nothing pushed; no
    rebase, no touch of the live checkout.

    Not covered, worth knowing: the project-context team launch is proven at
    the lib/dep level, not driven end-to-end (the mock-bridge spec opens the
    plain channel dialog, and a project-context e2e would be a new fixture);
    item 79(b) the Agents-screen owner/team filter and 79(d) the model set
    on the card not reaching the agent record are untouched — neither was in
    this brief.

80. **Keystone's first mission from inside Beekeeper (2026-08-28 11:4x, session `AgentTeams`, channel c60447f0…) — what it found by doing.** Seated by hand via Add provider: Architect (codex), Builder (claude), Runner (codex) under Keystone (claude). Keystone briefed all four over the relay, measured, and reported with a Pulse ledger. Findings, its and mine:
    (a) **Hired seats ran inside Brian's live checkout.** Add provider's working directory defaulted to `/Users/brian/Projects/beekeeper/beekeeper`; three seats (architect, builder, runner) got that cwd while the lead got a worktree. A seat must never run in the operator's hot checkout: Add provider seats default to a per-seat worktree (the one-session path already has the worktree field), and the provider refuses a seat whose cwd is the shared checkout the app runs from. I stopped the three seats (session.stop as founder) as soon as I saw it.
    (b) **Packs materialized as a union** — consequence of (a): all seats sharing one cwd got every role's skills in one `.agents/skills`; per-seat cwd fixes it, and the materializer should refuse a cwd another seat already owns.
    (c) **`bee sessions send --deliver steer` says accepted:true when the provider degrades steer to boundary** (Claude has no native steer): the sender is misinformed; the CLI must wait for and print the turn_degraded/turn_queued receipt (S2 exposes it).
    (d) **Pulse went to a project Keystone minted for itself** (30621:ede63017…:beekeeper, private): the seat had no project coordinate, so `bee pulse update` had no target. The provider must pass the umbrella's projectRef as BUZZ_PULSE_PROJECT to seats; Brian cannot see those six entries. Where Pulse shows: Projects → Bee Keeper → Pulse.
    (e) **The header says "Fizz · Lead" for Keystone**: the seat name comes from the identity's relay profile (kind 0 displayName) which still says Fizz; renaming an agent must republish its profile.
    (f) **Context: a codex seat spends ~25% of its window at boot; the mandatory ledger read would take it to ~48%.** The lead pack tells seats to read SESSION_STATE.md whole; ship a digest (the beekeeper-project skill should carry the rules, not point at a 2,700-line file).
    (g) Unseated roles (designer, poker, verifier) rejected client-side with no event — correct. Interrupt and readdress untested by Keystone, reasons on record.

    **Fixed f6a8c78f, 3781e058, 8c83bec2:** three lanes on
    `crew/front-door`, each lane's own report of what it delivered, where it
    deviated, and what it did not do:

    *Lane W — a hired seat gets its own tree, and the provider refuses
    somebody else's (f6a8c78f)*

    Delivered:

    - Desktop join dialog: seating an agent now reveals the founding path's
      worktree field (NewCodingSessionWorktreeField, reused unchanged),
      checked by default, prefilled `<session-slug>-<role>` via new pure
      `addCodingSessionProviderSeatWorktreeName` (composed through the
      host's own codingSessionWorktreeSlug, so it is idempotent and the
      field shows what the host would compute).

    - Desktop: the worktree is created before the command is signed
      (createCodingSessionWorktree), its path becomes the create's workdir,
      and the checkout — not the worktree — is what the recent-folders list
      learns; buildAddCodingSessionProviderSubmit gained `rememberWorkdir`
      in and out. A failed worktree create renders at data-testid
      add-coding-session-provider-worktree-error and publishes nothing.

    - Desktop: the dialog says which directory the seat will run in — new
      pure `addCodingSessionProviderSeatWorkdirNote` at data-testid
      add-coding-session-provider-workdir-note: worktree on -> "Ada runs in
      a new worktree made from <path> — its own directory and branch. It
      does not share that checkout's index, HEAD, or role skills."; worktree
      off -> "Ada runs directly in <path>, sharing its branch, uncommitted
      changes, and role skills with anything else running there."

    - Desktop: the plain (unseated) join is unchanged — no worktree UI, no
      note, same submit bytes; asserted by test.

    - Provider: new `session::seated_workdir_refusal(cwd, live,
      shared_roots)` + `LiveWorkdirClaim` + code `SEAT_CWD_SHARED`. Refuses
      a seated create whose cwd is a shared root (home/nest — now enforced
      for every seated create, not only one carrying a pack) or is the cwd
      of another live (not-closed) execution of the same umbrella; the
      sentence names the other seat ("the lead seat of this same session is
      already running there") and an unseated occupant reads "the person who
      opened this session" — the lead's own tree counts.

    - Provider: wired into Provider::create_session before any provisioning
      (crates/buzz-session-provider/src/lib.rs, ahead of rehydration and the
      custody read); the refusal calls forget_actor_seat, so the staged seat
      key does not survive it, and emits a failed 44224 receipt.

    - Red before green, all four: desktop model tests failed on missing
      exports; dialog tests failed 'Unable to find
      [data-testid="coding-session-worktree-toggle"]'; provider unit tests
      failed to compile on `seated_workdir_refusal`; lib test
      a_seated_create_sharing_a_live_executions_tree_is_refused_by_name
      failed `left: "created", right: "failed"` — the provider hired the
      seat straight into the lead's checkout.

    Deviations:

    - SEAT_CWD_SHARED is defined in
      crates/buzz-session-provider/src/session.rs, not beside the other
      lifecycle codes in crates/buzz-core/src/coding_session_payload.rs —
      that file is outside this lane's ownership. Consumers render the
      message (no code-specific UI mapping exists for it today); moving the
      const to buzz-core is a one-line follow-up if the CLI/desktop want to
      branch on it.

    - The create-path shared-root refusal reports SEAT_CWD_SHARED rather
      than the older PROVIDER_UNAVAILABLE. The deeper materialize-time
      refusal (materialize_seat_skills_outside) is untouched and still
      returns PROVIDER_UNAVAILABLE as a backstop, so its existing test is
      unchanged.

    - "Live" is read as `!record.closed` — the crate's own definition in
      SessionState::live_session_count — not "has a live actor handle in
      SessionManager". A crashed-but-not-closed execution therefore still
      holds its tree; that is the conservative direction.

    - The worktree defaults ON only for a seated join. An unseated join
      shows no worktree control at all (brief: keep today's behaviour),
      rather than showing it unchecked.

    Residuals:

    - Not driven end-to-end: the provider refusal is proven at the create
      path with seeded session records plus pure-function unit tests, and
      the dialog is proven in jsdom. No two-real-seats live run, and no
      relay-backed E2E.

    - The join's createCodingSessionWorktree call is not exercised by a test
      that goes through the Tauri bridge — outside Tauri the worktree
      plan/branches effects no-op, so the jsdom tests cover the field, the
      name, and the copy, not the actual worktree creation.
      desktop/tests/e2e is outside this lane's ownership, so no e2e spec was
      added.

    - If a seated join is submitted with an empty working-directory field,
      no worktree is made and the create carries `workdir: null`; the
      provider then resolves its own cwd and the new SEAT_CWD_SHARED guard
      is the only backstop. Worth a dialog-level block in a later pass.

    - The provider guard sees only executions this provider knows about. Two
      seats hired on two different provider hosts into one tree on a shared
      filesystem are not covered.

    - Item 80 (c)-(g) untouched: steer receipt honesty, BUZZ_PULSE_PROJECT,
      the Fizz/Keystone profile name, and the ledger-digest context problem
      are other lanes.

    *Lane P — three honesty fixes (seat pulse project, real turn delivery,
    agent profile republish) (3781e058)*

    Delivered:

    - (d) Seats get the umbrella's project: new
      ActorSeat::post_fence_env_in_project (actor_seats.rs) appends
      BUZZ_PULSE_PROJECT (new PULSE_PROJECT_ENV const) after the fence when
      the execution names a projectRef; blank/whitespace coordinates are
      treated as no project. Wired at both seat sites in lib.rs — create
      uses plan.project_ref, resume uses record.project_ref. Test
      a_seated_execution_targets_the_umbrellas_project asserts the variable
      appears with a coordinate, is absent for None/""/"   ", and that the
      pre-existing key/git list is unchanged and still first.

    - (c) bee sessions send prints the provider's answer, not only the
      relay's. Pure fold_delivery(requested, stage, waited) + DeliveryReport
      in crew.rs maps
      turn_queued/turn_degraded/turn_started/interrupt_delivered/turn_dropped/turn_refused
      into delivered (true/false/null), deliveryStatus (the receipt's own
      word or "unconfirmed") and delivery (one sentence). A degraded steer
      reads exactly "steer requested, provider degraded to boundary".
      crew_cmds.rs: publish_with split into submit_with + print; new
      await_delivery polls kind 44224 for this commandId (since = publish
      second − 1) every 500ms for DELIVERY_WAIT_SECONDS = 10, retrying read
      errors rather than failing a write that landed. accepted keeps its old
      meaning. New global --no-wait flag (lib.rs Send args, dispatched
      through sessions.rs) skips the read and says so — "nobody answered"
      and "nobody was asked" get different sentences. 4 tests, including the
      degraded-steer red and
      no_wait_says_it_did_not_look_rather_than_that_nobody_answered.

    - (e) The team-roles installer republishes each installed identity's
      kind:0 profile. New pure role_profile_publishes(previous_agents,
      &CrewRoleInstall) -> Vec<CrewRoleProfilePublish> in
      managed_agents/crew_roles.rs (carries pubkey, display_name,
      previous_name; no key material, so it can never log one).
      commands/crew_roles.rs plans the publishes inside the store lock
      (avatar falls back to the effective harness default exactly as a
      dialog rename does) and signs them outside it via the existing
      sync_managed_agent_profile path. Failures are reported in a new
      InstallCrewRolePacksResponse.profile_sync_error naming the identities
      the relay still knows by their old name, never swallowed and never
      fatal (the stores are already written). Test
      renaming_the_lead_owes_a_profile_publish_with_the_new_name covers
      Fizz→Keystone plus the unrenamed builder.

    - Runbook updated: crates/buzz-cli/TESTING.md now shows the steer
      example's four output fields and the --no-wait sentence.

    Deviations:

    - Touched crates/buzz-cli/src/commands/sessions.rs (2 lines) — not in
      the owned list, but adding no_wait to the SessionsCmd::Send variant
      makes the exhaustive destructure at :2295 a compile error otherwise.
      Pure mechanical pass-through.

    - Touched desktop/src-tauri/src/commands/crew_roles.rs (the installer
      command) and desktop/src-tauri/src/managed_agents/crew_roles_tests.rs
      (the test home for crew_roles.rs). The brief named commands/agents.rs
      plus 'the profile-publishing module you find'; the publish belongs in
      the installer command, and its test belongs beside the installer's
      other tests.

    - Did NOT touch desktop/src-tauri/src/commands/agents.rs. Renaming an
      agent from the dialog ALREADY republishes its kind:0 —
      desktop/src-tauri/src/commands/agent_models_update.rs:227-250 builds
      the sync params on name_changed and :290-330 syncs, rolling the rename
      back if the publish fails. The only gap for finding (e) was the
      team-roles installer, so that is where the fix went.

    - Touched crates/buzz-cli/TESTING.md to document the new send output; a
      runbook that no longer describes what the command prints is its own
      small untruth.

    - The receipt wait applies to every --deliver class, not only
      steer|interrupt (the brief's title scoped it to those two). A boundary
      send now also reports turn_queued/turn_started rather than only
      accepted:true; --no-wait restores the old behaviour. Typical added
      latency is one 500ms poll.

    - Added post_fence_env_in_project as a second method instead of changing
      post_fence_env's signature, because
      crates/buzz-session-provider/src/session.rs:2818 calls it and lane W
      owns that file. post_fence_env is unchanged and still the
      identity-only list.

    Residuals:

    - (d) does not yet reach a TEAM-launched seat.
      desktop/src/features/coding-sessions/ui/useCodingSessionCrewLaunch.ts:143
      publishes every seat's create with projectRef: null, so
      plan.project_ref is None and no coordinate is exported. The
      Add-provider/one-session seated path DOES carry it
      (useNewCodingSessionCreate.ts:546), which is the path Keystone was
      seated through, so the fix lands there. Threading
      projectContext.projectRef into the team launch is a one-line TS change
      in a file this lane does not own.

    - The install dialog does not render the new profileSyncError — the
      field is on the response and serialized as profileSyncError, but the
      TS surface (desktop/src) is not owned by this lane, so a failed
      republish is currently visible only to a caller that reads the
      response.

    - Ledger 80 (a) hired seats running in Brian's live checkout, (b) pack
      union from the shared cwd, and (f) the lead pack's whole-ledger read
      are untouched — not in this lane's brief.

    - await_delivery re-reads the channel's receipts since the publish
      second on each poll (up to 20 queries over 10s). Bounded by `since`,
      but a channel with heavy receipt traffic in that window pays for it; a
      per-commandId filter would need relay-side support.

    *Lane D — role packs stop sending seats at the whole ledger; "Crew" →
    "Team" in pack display names (8c83bec2)*

    Delivered:

    - beekeeper-project SKILL.md: the mandatory whole-ledger read is gone.
      Old line 9 said docs/SESSION_STATE.md 'wins — read it first, every
      session'; the new '## The ledger — read §3, and only the items your
      brief cites' section names §3 Next plus the numbered §2 items the
      brief cites, cites ledger item 80f for why (codex seat ~25% at boot,
      whole file would take it to ~48%), and ships the commands to jump
      straight there.

    - The skill now carries the operator's rules itself, and every
      restatement of AGENTS.md is a pointer instead: git basics → the
      AGENTS.md header block, quality gates → § Quality Gates and § Text
      sizing & zoom, working agreements → § Working agreements, event kinds
      and h-tag scoping → § Key Patterns. What stays in full is exactly what
      AGENTS.md does not say: push to origin only (bridge race), the item-71
      HTTP-401-after-green-hooks retry rule, the hot main checkout (item
      80a), wire kinds 44220–44230 and per-stage receipts, D11–D16, 'crew'
      is 'team', 'approving on a report alone', 'absence is not a claim'.

    - write-brief: new 'Ledger: §3 Next, plus items <numbers>' field in the
      locked template, plus a rule section — a lead names the items a lane
      needs; if it cannot, the brief is not ready to write, not a licence to
      hand over the whole file.

    - triage-report and lead.persona.md: the same scoping, so a lead reads
      the ledger the way it makes its lanes read it, and every published
      disposition cites the item number it settles.

    - Display names renamed "Crew <Role>" → "Team <Role>" on architect,
      builder, designer, poker, runner, verifier (lead was already "Team
      Lead"), with keywords "crew" → "team". Plugin ids
      (com.beekeeper.crew.*) unchanged. No description contained "crew".
      Original inline-array JSON formatting preserved — each file is a
      2-line diff.

    - Line counts before → after: lead/skills/beekeeper-project 114 → 122;
      lead/skills/write-brief 40 → 56; lead/skills/triage-report 56 → 58;
      lead/personas/lead.persona 71 → 73; lead/skills/choose-model 53 → 53
      (untouched); verifier persona 33 → 33; all six plugin.json 9 → 9.
      Every other pack file unchanged. Max is 122, under the 150 ceiling.

    - Net context economics: the whole lead pack is 18,723 bytes. §3 Next is
      14,065 bytes; the full ledger is 262,337. The pack grew ~1.0 KB and
      removed up to ~248 KB from a lead's mandatory read.

    Deviations:

    - The brief says "§3 Next (top of the ledger)". It is not at the top —
      §3 starts at line 3102 of 3716 (grep -n '^## ' docs/SESSION_STATE.md).
      I wrote the instruction accurately and gave the seat the commands to
      jump there rather than scroll: grep -n '^## ' for section lines, sed
      -n '/^## 3\. Next/,/^## 3a\./p' for the track (~195 lines), grep -n
      '^79\. ' then a sed window for one item.

    - Added a warning I found by testing my own instruction: ranging an item
      to the next number (awk '/^79\. /,/^80\. /') returns 753 lines because
      item 80 does not exist yet and the range runs to EOF — i.e. the naive
      extraction hands back most of the file. The shipped commands use grep
      -n + a bounded sed window instead.

    - De-crewed prose beyond plugin.json: verifier.persona.md said "You are
      the crew's refuter" and "A crew is only as honest as its cross-checks"
      — both are read by a person/seat, so the operator's own rule applies.
      Now "the team's refuter" / "A team is only as honest". The only
      remaining "crew" in personas/roles/** is the rule stating the rule
      (beekeeper-project:114) and the unchanged plugin ids.

    - The brief scoped the ledger-read fix to "personas/roles/lead ... and
      every other role pack that says so". No other pack says so — grep for
      SESSION_STATE/docs/ across personas/roles found the instruction only
      in lead/skills/beekeeper-project:9. So the lead pack was the whole
      surface, and I added the scoping discipline to the three other lead
      files the brief named.

    - The tauri test needed sidecar stubs to compile (build.rs panics on
      missing binaries/buzz-acp-aarch64-apple-darwin). Ran the repo's own
      `just _ensure-sidecar-stubs`, which creates untracked, gitignored stub
      files inside my worktree only. git status is clean apart from my owned
      files.

    Residuals:

    - No automated test asserts the pack display names, so "Team Architect"
      is unguarded: `pack validate` does not check it, and the crew_roles
      scan keys off persona frontmatter `role:`, not plugin.json `name`. A
      guard would live in
      desktop/src-tauri/src/managed_agents/crew_roles*.rs, which I do not
      own.

    - I could not find any consumer of plugin.json `name` in the product:
      grep for 'Crew Architect|Crew Builder|Crew Runner|Crew Verifier|Crew
      Designer|Crew Poker' across .rs/.ts/.tsx/.json/.md returned only
      docs/SESSION_STATE.md:2720 (the record of the earlier lead rename) and
      card.rs:355 (an example string). So these display names may be inert
      metadata today — the rename is correct either way, but nobody has
      shown it rendering.

    - The absolute numbers in the skill (~3,700 lines, §3 at ~3102) will
      drift as the ledger grows. Mitigated by telling the seat to run `grep
      -n '^## '` rather than trust the number, but the prose figure will
      age.

    - Not verified: whether an actual codex seat's boot+read percentage
      improves, since that measurement (item 80f) came from a live Keystone
      run I cannot reproduce here. The claim in the pack is attributed to
      item 80f rather than asserted as my own measurement.

    - Item 80 is still only in
      /Users/brian/Projects/beekeeper/review-2026-08-28/ledger-80-draft.md,
      not in docs/SESSION_STATE.md. The pack cites "ledger item 80f", which
      is a forward reference until that draft lands.

    Gate, run on the integration branch before landing (all green): (1)
    `cargo test -p buzz-cli -p buzz-session-provider -p buzz-persona -p
    buzz-core -p buzz-acp --lib` — 865+577+442+157+394 = 2,435 passed, 0
    failed across 5 binaries; (2) `cargo clippy --workspace --all-targets --
    -D warnings` clean, exit 0; (3) `cargo fmt --all -- --check` clean, exit
    0; (4) desktop `pnpm typecheck && pnpm test` — tsc clean, 6,532 passed /
    0 failed; (5) `cargo test --manifest-path desktop/src-tauri/Cargo.toml`
    — 2,748 passed, 0 failed, 18 ignored (doc-tests, csp and rodio suites
    also 0 failed); (6) `pnpm check:px-text` clean, exit 0; (7) `just
    file-size-check` — 9/9 node subtests plus desktop/web/mobile size
    scripts clean, exit 0; (8) `pack validate` x7 (lead, architect, builder,
    runner, verifier, poker, designer) — all "Valid.", exit 0 each; (9)
    desktop `pnpm build:e2e` + `playwright crew-front-door.spec.ts
    --project=smoke` — build succeeded, 8 passed (18.0s), exit 0.

81. **D14 hire — the lead brings agents in (built 2026-08-28 on
    `crew/front-door`, commits `c3db10f0`, `19664da3`, `e11ae068`,
    `3c9e2d53`).** Launching a team now seats the lead and nobody else; the
    roster it sees is the seats it may *hire*. A lead publishes a
    `session.hire` action on kind 44221, the relay authorizes it (founder or
    granted operator only), and the founder's desktop answers it by seating an
    identity whose home role matches. Three lanes built it; each lane's own
    report of what it delivered, where it deviated, and what it did not do
    follows.

    *Lane C — the wire and the CLI (`session.hire`) (`c3db10f0`)*

    - buzz-core: CodingSessionLifecycleAction::SessionHire — exactly seven keys
      {type, sessionRef, genesisRef, role, providerInstanceRef, model, brief},
      one accepted form only (no historical shapes, because the action is new
      with the relay that validates it). providerInstanceRef/model nullable but
      structurally present; genesisRef required non-null (a create's is
      optional) because a hire is authorized against the genesis; brief
      1..MAX_LIFECYCLE_INITIAL_TURN_BYTES (12288) since it becomes the seat's
      first turn; role reuses validate_role_slug. New public
      HIRE_REFUSAL_PREFIX ("hire refused: ") and HIRE_REFUSAL_CODES (HIRE_OFF,
      HIRE_ROLE_NOT_ALLOWED, HIRE_LIMIT, HIRE_NO_IDENTITY,
      HIRE_PROVIDER_NOT_ALLOWED) so the desktop and the CLI agree on a
      refusal's shape. New
      CodingSessionLifecycleCommandPayload::hire_session_ref().

    - buzz-core red-before-green: 5 new tests, watched compile-red (`variant
      SessionHire not found` x3, `no method named hire_session_ref` x2) then
      green — accepts_exactly_the_seven_key_hire_action,
      only_a_hire_names_a_hire_session_ref,
      refuses_a_hire_with_extra_missing_or_null_required_keys,
      refuses_a_hire_whose_role_is_not_a_slug,
      refuses_a_brief_past_the_initial_turn_ceiling.

    - buzz-relay: the hire is the one lifecycle action the relay must authorize
      itself, because it never reaches a provider. New
      KIND_CODING_SESSION_LIFECYCLE_COMMAND arm in
      check_coding_session_membership (ingest.rs), with two pure helpers:
      hire_umbrella_of(event) reads the umbrella out of the action (the 44221
      envelope is exactly h/csl-v/csl-command and has no room for a fourth tag;
      content that does not decode is not a hire) and
      hire_authority_verdict(Option<&SessionAuthority>, pubkey) requires
      founder-or-operator via the same SessionAuthority::may_steer the 44227
      goal rule uses. Message on refusal is exactly "restricted: only the
      session founder or a granted operator may hire". The gate's doc comment
      gained a 44221 bullet.

    - buzz-relay red-before-green: 2 new tests, watched red (`cannot find
      function hire_umbrella_of/hire_authority_verdict`, 8 errors) then green —
      only_a_session_hire_names_an_umbrella_the_relay_must_authorize (a create
      and unparseable content both report None) and
      a_hire_is_refused_unless_the_signer_founded_or_was_granted_the_umbrella
      (founder ok, operator ok, viewer refused, stranger refused, unknown
      umbrella refused by its own sentence).

    - buzz-sdk: no new builder needed (creates have none either —
      build_coding_session_lifecycle_command is generic over the payload).
      Added a_lifecycle_command_builder_signs_a_session_hire: three ordered
      tags, round-trip through the relay's strict decoder, and a non-slug role
      refused at the builder.

    - buzz-cli: `bee sessions hire --channel <uuid> --session-ref <uuid> --role
      <slug> [--genesis <hex>] [--provider-instance <ref>] [--model <id>]
      (--brief <file> | --content <text>) [--no-wait]`. --genesis is resolved
      from the channel's 44226 when omitted (two geneses claiming one label is
      an error listing both, never a coin flip). Publishes, then waits up to
      HIRE_WAIT_SECONDS=60 (2 s polls) and folds what the channel says into one
      printed JSON document: `accepted` stays the relay's fact,
      `outcome`/`detail`/`seat`/`code`/`reason` are the host's.

    - buzz-cli pure folds in crew.rs, all unit-tested: find_hired_seat (the
      answer is recognized by what it is — a seated create for this role, in
      this umbrella, after the request; the request carries no id the answer
      echoes), newest_create_receipt (non-turn receipts only),
      parse_hire_refusal (structural: prefix + [A-Z0-9_]+ code +
      em-dash-or-hyphen + non-empty reason, so an agent merely talking about a
      refused hire does not parse), find_hire_refusal (scoped to executions the
      channel says belong to this umbrella, and to after the request),
      fold_hire (a published seat outranks a refusal), hire_report,
      hire_exit_code, hire_unsupported_by_relay, hire_payload,
      resolve_umbrella_genesis.

    - buzz-cli WIRE RULE: hire_unsupported_by_relay recognizes the three shape
      errors an older relay answers with ("action type is unsupported", "has
      missing or unsupported fields", "malformed … payload") and turns them
      into `this relay does not accept hire requests yet — it validates kind
      44221 against a closed action list that has no `session.hire` in it … The
      relay said: <the relay's own words>`, raised as CliError::Relay{400} →
      exit 2. The relay's sentence is kept, not replaced.

    - buzz-cli exit codes: 0 created, 1 refused (host policy) or failed
      (provider refused the seated create), 2 relay error, 5 seating (a create
      with no receipt inside 60 s) or unconfirmed (nothing answered, or
      --no-wait). `seating` is deliberately its own word and its own sentence —
      a seat exists but nothing says it runs.

    - buzz-cli red-before-green: 9 new tests in crew_tests.rs, watched red (37
      compile errors, `cannot find … HireRefusal/find_hired_seat/fold_hire/…`)
      then green, including a_hire_publishes_the_seven_key_action_byte_for_byte
      (a literal string comparison of the serialized payload).

    - Docs: NIP-CSL.md gains a `session.hire` fork amendment (wire shape, why
      genesisRef is required, relay-checked authority and why, the host's two
      answers with the seated create's receipts being the hire's receipts, the
      refusal turn's exact text, and the deployment-order rule); the existing
      'additive v1 evolution' paragraph now names hire as what the fail-closed
      rule makes safe. crates/buzz-cli/TESTING.md gains a hire runbook block, a
      recorded live run table, and checklist row 70.

    - Live evidence (relay built from this branch, 127.0.0.1:3077, docker
      pg/redis, founder 88cfb21c…, granted operator 8b2bd4e6…, channel
      ddcccba6-893b-4fcb-bf29-543ddecc260d, umbrella 5b7e1c2a-…-7c2d8e6f4a10,
      genesis b13cbd5f…): founder hire accepted (event 3fe71e03…); the stored
      content read back out of Postgres is
      `{"type":"session.hire","sessionRef":…,"genesisRef":…,"role":"builder","providerInstanceRef":null,"model":null,"brief":…}`;
      a granted collaborator's hire accepted (3ad96681…); a stranger refused
      400 `restricted: only the session founder or a granted operator may
      hire`; a forced --genesis into an unclaimed umbrella refused `restricted:
      no coding-session genesis in this channel claims that sessionRef…`; a
      seeded seated create + `created` receipt produced outcome=created, seat
      8b2bd4e6·runner, exit 0; a seeded refusal turn produced outcome=refused,
      code HIRE_OFF, exit 1; --no-wait produced outcome=unconfirmed, exit 5.

    Lane C deviations:

    - Three files outside the named ownership were touched, all forced by
      adding an enum variant or a subcommand, and all minimal. (1)
      crates/buzz-core/src/pulse_fold.rs — two match arms: a hire never
      succeeds a receipt (it produces none of its own) and names no provider
      authority (empty string matches no signer). (2)
      crates/buzz-session-provider/src/commands.rs — decide_lifecycle now
      returns Ignore(NotAddressed) for a hire, because a hire is addressed to
      the umbrella's host and names no providerAuthorityPubkey; the second,
      guarded match's `SessionCreate { .. } => unreachable!()` became `_ =>
      unreachable!()`. (3) crates/buzz-cli/src/commands/sessions.rs — one
      dispatch arm for SessionsCmd::Hire (the clap variant is in lib.rs, the
      match is here, so the enum cannot be extended without it).

    - crates/buzz-cli/src/error.rs gained two variants, CliError::Refused (exit
      1, category "refused") and CliError::Unconfirmed (exit 5, category
      "unconfirmed"). The brief's exit table needs 1 for a refusal and 5 for an
      unconfirmed hire, and the existing variants that carry those codes are
      Usage/NotFound ("user_error"/"not_found") and Conflict ("conflict") —
      every one of which would have printed a category that lies about what
      happened. error.rs has no exhaustive CliError match outside itself and
      has not been touched since the 2026-08 rebrand, so the change is additive
      and low-conflict.

    - An umbrella that no genesis in the channel claims is REFUSED for a hire,
      where the 44227 goal rule falls back to base channel membership for an
      unclaimed label. Reason: the goal rule protects legacy signers, and a
      hire has none — it is new with the relay that validates it — so falling
      back would let any channel member spend the founder's machine. The
      refusal has its own sentence naming the missing genesis, distinct from
      the founder-or-grant one.

    - The CLI takes `--genesis` as an optional override and resolves it from
      the channel by default. The brief's signature did not list it, but the
      wire requires a non-null genesisRef; resolving is the better default and
      the flag exists for the ambiguous case (two geneses on one label, which
      NIP-CSG explicitly allows a relay to store).

    - `--brief <file>` uses read_file_or_stdin and `--content <text>` uses
      read_or_stdin, matching the brief's `(--brief <file> | --content <text>)`
      split. `bee sessions create --brief` treats its argument as literal text;
      the two flags now differ across the two subcommands. Documented in both
      help strings.

    - A fifth outcome word, `seating`, exists beyond the brief's
      created/failed/refused/unconfirmed: a seated create published with no
      provider receipt inside the wait. It exits 5 with the rest of the
      unconfirmed family, but says something different, because 'a seat exists
      and nothing has said whether it runs' is not the same fact as 'nothing
      answered at all'.

    - The live run used two throwaway `cargo run --example` seeders (in
      crates/buzz-cli/examples/) to play the host and the provider — no host
      implements session.hire yet. Both files were deleted before the commit;
      nothing of them is in the diff. The dev Postgres now holds one extra
      community row (host 127.0.0.1:3077), one channel, and the seeded session
      events.

    Lane C residuals:

    - No host implements session.hire. Everything downstream of the relay — the
      standing policy (hiring on/off, allowed roles, max live seats, allowed
      providers), identity selection by home_role, the per-seat worktree,
      custody staging, the seated create with initialTurn prefixed "[From the
      lead] ", the refusal turn and its umbrella system line — is another
      lane's. Until it lands, every hire on a real relay ends `unconfirmed`
      after 60 s.

    - The `failed` outcome (a provider receipt refusing the seated create) and
      the `seating` outcome are unit-tested only; the live run covered created,
      refused and unconfirmed.

    - The "this relay does not accept hire requests yet" path is unit-tested
      only — proving it live needs a relay built before this branch.
      hire_unsupported_by_relay keys on three substrings of the relay's
      rejection message; if the relay's wording changes, the sentence degrades
      to passing the shape error through, which is why the relay's own words
      are always appended.

    - A refusal turn is bound to its request by (umbrella, after-the-request,
      structural text), not by an id: the 44220 payload has no field to echo
      the hire's commandId, and the brief pins the refusal's exact copy. Two
      hires into one umbrella inside the same window could in principle read
      each other's refusal. Fixing it properly needs a field on the refusal — a
      contract change, not this lane's.

    - The wait polls four kinds over the whole channel every 2 s for up to 60 s
      (30 reads). It is bounded but not cheap on a busy channel; a `since`
      bound like the one `bee sessions send` uses is only possible for the
      receipt kind, since the seated create must be matched by content.

    - find_hire_refusal only sees executions the channel's 44223 metadata
      describes. A refusal addressed to a seat whose metadata has aged out, or
      that never published any, is invisible and the hire reads `unconfirmed`.

    - newest_create_receipt does not fence on the create's
      providerAuthorityPubkey the way build_founder_index's joined_target does
      — any signer's receipt for that commandId is read. Tightening it is a
      small follow-up; the looser read cannot manufacture a seat that does not
      exist, only report someone else's claim about one.

    - The relay resolves the hire's umbrella by decoding event content inside
      the ingest gate. That is one extra decode per 44221 on the write path
      (the envelope validator decodes again a few steps later). Measured cost
      not taken.

    - The desktop is untouched by this lane: nothing in the app can send or
      show a hire yet, and the umbrella has no system line for a refusal.

    - The dev database used for the live run keeps its seeded rows (community
      127.0.0.1:3077, channel ddcccba6…, one genesis, two hires, one seated
      create, one receipt, one metadata, one refusal turn, one 44228 grant).
      Harmless dev data; delete the community row if the clutter matters.

    *Lane H — the founder's desktop honours hires (`19664da3`)*

    - Wire (lib/codingSessionHireWire.ts): the 44221 `session.hire` action with
      exactly the seven contract keys in order
      (type/sessionRef/genesisRef/role/providerInstanceRef/model/brief), tags h
      / csl-v csl1-1 / csl-command. buildCodingSessionHireEvent +
      validateCodingSessionHireInput refuse every bound before signing;
      classifyCodingSessionHireEvent accepts the exact key set or returns
      malformed. A `session.create` on the same kind classifies `irrelevant`,
      not malformed, so ordinary sessions never inflate a malformed count.

    - WIRE RULE:
      describeCodingSessionHireFailure/isCodingSessionHireUnsupportedRelayFailure
      translate the relay's `malformed coding-session lifecycle command
      payload` rejection into CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE —
      "This relay does not accept hire requests yet — it refused the request as
      malformed. The relay has to ship session.hire before a lead can hire a
      seat." publishCodingSessionHire rejects with that sentence already
      applied, so no caller can surface the raw JSON wording. Every other
      failure keeps its own words.

    - Policy (lib/codingSessionHirePolicy.ts): CodingSessionHirePolicy
      {enabled, allowedRoles|null, maxSeatsPerUmbrella,
      allowedProviderInstanceRefs|null}, default {true, null, 4, null},
      persisted in localStorage under buzz.codingSessions.hirePolicy.v1 (same
      device-preference pattern as model favourites / session width).
      decideCodingSessionHire produces exactly one of the five contract codes:
      HIRE_OFF, HIRE_ROLE_NOT_ALLOWED, HIRE_LIMIT, HIRE_PROVIDER_NOT_ALLOWED,
      HIRE_NO_IDENTITY (whose reason names the remedy "Install team roles").
      formatCodingSessionHireRefusal renders `hire refused: <code> — <reason>`
      exactly as the contract spells it.

    - Identity choice: codingSessionHireAllowedRoles derives the default role
      list from installed packs (homeRole present and hasRolePack !== false;
      `undefined` subtracts nothing, since "nobody asked" is not "no pack").
      Selection takes only agents whose home_role IS the hired role (D12),
      skips any already live in that umbrella, and orders deterministically by
      pack-staged, then name, then pubkey. Every builder already seated yields
      HIRE_NO_IDENTITY rather than a duplicate seat.

    - Seat (lib/codingSessionHireSeat.ts + codingSessionHireAnswer.ts):
      buildCodingSessionHireSeatPlan produces exactly what
      buildCodingSessionCreateEvent accepts — actor, role,
      sessionRef/genesisRef, inherited title, initialTurn = "[From the lead] "
      + brief (not doubled if the lead already wrote the prefix),
      providerInstanceRef = request's or the policy default, model = request's
      or the identity's. Per-seat worktree name `<session-slug>-<role>-<n>` via
      the host's own codingSessionWorktreeSlug, ordinal counting the seats of
      that role already in the umbrella. listCodingSessionHireLiveSeats reads
      live seats from the umbrella (ended = completed/stopped/failed;
      disconnected and interrupted still hold their seat).

    - Authority: isCodingSessionHireAuthorized — founder or a live
      grant-operator, same rule as steer. An unresolved founder authorises
      nobody (the permissive fallback used elsewhere would here read as "anyone
      may spend this computer"). planCodingSessionHireAnswer *ignores* an
      unauthorised or unknown-umbrella request rather than refusing it, so a
      stranger cannot make this host sign events on demand.

    - Hook (hooks/useCodingSessionHire.ts): subscribes through
      subscribeToObservedCodingSessionEvents — the existing create-observation
      fan-out bus — so no second relay connection is opened. Dedupes by
      commandId and marks answered *before* the effect, so a history replay on
      reconnect cannot seat the same agent twice. Reuses
      createCodingSessionWorktree + stageCodingSessionCreateHint,
      publishSeatedCodingSessionCreate with ensureActorChannelMembership /
      stageCodingSessionActorSeat / clearCodingSessionActorSeat,
      fetchCodingSessionRosterFold for grants, publishCodingSessionCommand for
      the refusal turn to the requesting seat, and
      publishCodingSessionLaneMessage for the umbrella's visible line.

    - Settings (features/settings/ui/CodingSessionsSettingsPanel.tsx): a new
      "Hiring" group with CodingSessionHiringCard — on/off switch, allowed
      roles (only roles whose packs are installed here), live seats per
      session, allowed providers (only runtimes whose authState is `ready`).
      Every control names the refusal code it produces; the off state says
      "every hire request is refused HIRE_OFF, and the lead is told so" rather
      than just reading disabled.

    - D14 lead-only launch (lib/codingSessionCrewLaunch.ts +
      ui/NewCodingSessionCrewTab.tsx): launchCodingSessionCrew now publishes
      exactly ONE seated create — the lead's — and reports the rest as
      `hireableSeats`. planCodingSessionCrewLaunch emits one create step. The
      model/vendor checks are scoped to the seat actually created, which
      retires the item 79(c) block: a Codex architect no longer disables
      Launch. New codingSessionCrewLeadFirstTurnText gives the lead the goal,
      the roster labelled as who it may hire, the `bee sessions hire` command,
      and the rule that a hired seat's first turn IS the brief. The roster
      renders data-seat-state="seated"/"hireable" with "· not launched — the
      lead may hire it", and CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE says
      "Launching seats the lead only" above the button.

    - Red before green, watched on this host: the exact-key rule watched red by
      deleting it (classifier returned `hire` where `malformed` was expected);
      codingSessionHirePolicy/HireSeat/HireAnswer suites all failed on the
      missing module before it existed; the four D14 launch tests failed 4/23
      against the multi-seat launch; the roster seated/hireable test failed on
      the missing CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE export.

    Lane H deviations:

    - desktop/src/shared/api/types.ts was NOT touched: ManagedAgent already
      carries `homeRole: string | null` and `hasRolePack?: boolean`
      (types.ts:236,244), which is everything identity selection needs. No
      optional field was required.

    - desktop/src-tauri/src was NOT touched: no new Tauri command was needed.
      The policy is a device preference read by the desktop's own TS, so it is
      persisted in localStorage like the session-width and model-favourite
      preferences, not through the provider settings store the turn budget uses
      (that one has to reach the Rust supervisor; this one does not).

    - The umbrella refusal is published as a session-lane message (kind:9 +
      cs-session tag, via the existing publishCodingSessionLaneMessage) rather
      than as a lifecycle system row. The brief asked for "the existing
      system-row rendering"; that rendering lives in
      desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx
      (line 809), which is outside this lane's ownership and is 988/1000 lines
      — adding a branch there risked both an ownership breach and the file-size
      gate. The lane message lands in the same umbrella timeline as a
      conversation row and is signed by the operator, which is true. Converting
      it to a lifecycle row is a small follow-up in whichever lane owns that
      file.

    - The launch's family check (verifier vendor != every builder vendor, D8)
      no longer fires at launch, because neither seat is created there any
      more. It is not repealed — it moves to hire time. Pinned by the rewritten
      test "a same-vendor verifier no longer refuses a launch that never
      creates it". The check that DOES still guard the created seat is the
      runtime-vendor conflict (a seat declaring a vendor the selected runtime
      cannot run), pinned by "a created seat on a vendor the selected runtime
      cannot run is refused".

    - The `default` model alias is no longer a hard refusal for a one-seat
      launch. The old refusal existed only so the verifier rule would not check
      a model nothing ran; with no verifier created at launch there is no
      cross-seat rule left for the alias to fool, and refusing would block
      every runtime without live model discovery. Documented in the rewritten
      test and in the launch module's header.

    - codingSessionCrewLaunch.test.mjs had 11 tests encoding the multi-seat
      contract; each was rewritten (not deleted) against the seat the launch
      actually creates, using a new PRIMARY_BUILDER fixture where the check
      under test only applies to builder/verifier roles. Two were superseded
      outright by the new D14 tests ("a launch is receipt-gated", "the first
      turn carries the goal and the roster") and their content is now covered
      by "a launch publishes exactly one seated create" and "the lead's first
      turn carries the goal and the roster it may hire from".

    - No relay-side, CLI-side, or lead-pack work was done — those are other
      lanes. Nothing in this branch publishes a hire from the desktop UI;
      publishCodingSessionHire exists as the canonical encoder plus the
      wire-rule failure mapping, tested against a mock publisher.

    Lane H residuals:

    - THE HOOK IS NOT MOUNTED. useCodingSessionHire is written, typechecked,
      and its whole decision path is tested through planCodingSessionHireAnswer
      — but nothing renders it, so a running desktop does not yet honour a
      hire. Mounting needs app-level plumbing outside this lane: a host
      component (features/coding-sessions/ui/) rendered from app/AppShell.tsx
      (beside NewCodingSessionDialogHost at :922), supplying channelIds from
      the session-transport channels, operatorPubkey from the identity query,
      umbrellas from groupCodingSessionCatalog over
      useGlobalCodingSessionCatalog, checkoutForChannel from
      getCodingSessionWorkdirState (byProject ?? byChannel ?? mru[0]), and
      targetForActor from the same catalog. I did not take those two files.
      Until that lands, the lane's headline claim is machinery, not behaviour.

    - The hook itself has no test. Its rules are all in tested pure modules,
      but the effect sequence (worktree → hint → membership → custody → sign →
      publish; refusal turn + lane message) is proven only by typecheck. A
      jsdom test with injected deps is the natural follow-up, and would want
      the deps injectable — today they are imported directly, matching
      useCodingSessionCrewLaunch's shape.

    - No live exercise. No 44221 carrying `session.hire` was seen on a wire, no
      relay refused one, no seat was hired on this machine. The "this relay
      does not accept hire requests yet" sentence is proven against a mock
      publisher throwing the relay's exact string, not against a real relay.

    - When the host knows no checkout for an umbrella's channel, the hired
      seat's create is published with no workdir hint and no worktree; the
      provider then resolves its own cwd and lane W's SEAT_CWD_SHARED guard is
      the only backstop. The five contract refusal codes have no member for
      "this host has no directory to cut a tree from", so the failure surfaces
      through the create's own failed receipt rather than as a hire refusal.
      Worth a sixth code, or a policy-level default checkout.

    - The refusal lane message says "A seat asked to hire a <role>" — it does
      not resolve the requester's display name, because the operator-profile
      resolver is on another surface. codingSessionHireRefusalNotice takes
      requesterLabel, so wiring a real name is a one-line change at the call
      site.

    - No Playwright coverage for the Hiring settings card or for the changed
      Team tab (desktop/tests/e2e is outside this lane).
      desktop/tests/e2e/crew-front-door.spec.ts asserts Team-tab copy that this
      lane rewrote — I did not read or run it, so it may need updating; whoever
      owns that file should re-run pnpm test:e2e:smoke. (Closed by `3c9e2d53`:
      the spec was updated to the D14 disclosure and the smoke project passes
      8/8.)

    - No relay-backed check that the desktop's exact-key hire classifier and
      buzz-core's deny_unknown_fields decoder agree byte-for-byte — they are
      written to the same seven keys, but nothing cross-checks them. A
      conformance test belongs in the relay lane.

    *Lane K — the lead learns to hire (`e11ae068`)*

    - personas/roles/lead/skills/hire/SKILL.md (new, 113 lines): when to hire
      (a lane with exclusive file ownership you can brief in one file) and the
      three cases not to (a seat already holds the role → sessions send; a
      question, not a lane; two lanes over one file); the three decisions
      before the command (role — home role is fixed per identity, so no hiring
      a builder 'as an architect'; model/vendor via skills/choose-model with
      the one-clause reason; the brief written to a file).

    - Exact command block per the contract: bee sessions hire --channel
      <channel-uuid> --session-ref <umbrella-uuid> --role <slug>
      [--provider-instance <ref>] [--model <id>] (--brief <path> | --content
      <text>) [--no-wait], with each flag's default spelled out (omit
      --provider-instance → operator default; omit --model → the identity's
      own; brief 1..12288 bytes; --no-wait only when you will not act on the
      outcome, otherwise it waits 60 s) and exit codes 0 hired / 1 refused / 2
      relay error / 5 unconfirmed — with 'on 5, bee sessions list before you
      re-run, or you hire twice'.

    - Brief-as-first-turn rule stated three ways: the host publishes the create
      with the brief prefixed '[From the lead] '; do NOT send a second
      start/here-is-your-brief turn; END YOUR TURN after hiring (same rule as
      dispatch — polling sessions inbox inside the hiring turn buys nothing and
      invents duplicates); a correction is one sessions send, never a re-hire.

    - Refusal section: refusals arrive both as exit 1 and as a 44220 turn
      reading 'hire refused: <code> — <reason>' plus a system line in the
      umbrella, seat never created. Table of all five codes with the remedy
      that clears each — HIRE_OFF (ask the operator to turn hiring on, do not
      retry), HIRE_ROLE_NOT_ALLOWED, HIRE_LIMIT (default ceiling 4 — close a
      seat or ask to raise it), HIRE_NO_IDENTITY (ask the operator to Install
      team roles, or the only identity with that role is already live here),
      HIRE_PROVIDER_NOT_ALLOWED (name an allowed provider or drop the flag).
      Plus: never retry unchanged; publish the code as a blocker Pulse entry
      then change the request or BLOCK with the missing input.

    - WIRE RULE honesty section: lifecycle payloads are validated with exact
      keys, so a relay predating session.hire rejects the request as malformed
      and the CLI says 'this relay does not accept hire requests yet' (exit 2)
      — nothing reached the operator, no seat was considered; report it so the
      operator deploys a relay carrying the action, seat by hand via Add
      provider meanwhile, and never read that sentence as 'the role is
      unavailable'.

    - 'What a hire cannot do': cannot mint an identity (custody stays with the
      host, the lead never sees or passes a key); cannot choose a working
      directory (host makes <session-slug>-<role>-<n>); cannot reach an
      umbrella it does not lead (founder or granted operator, same rule as
      steer).

    - lead.persona.md: skills frontmatter lists ./skills/hire/; verb 2 (Brief)
      now says to hire with skills/hire when the lane has no seat yet and that
      the brief file IS the hire's first turn; the Hiring section opens with
      'launching a team seats you and nobody else; the roster you see is the
      seats you may hire', carries the command, and points at skills/hire for
      refusals and the end-your-turn rule.

    - write-brief/SKILL.md: the template's Seat: line gains the alternative
      'hire: <role> on <provider>/<model> — when no seat holds this lane yet';
      the Dispatch line gains the matching bee sessions hire --brief <this
      file> form; and a paragraph under 'The dispatch line is part of the
      brief' says the brief is the new seat's first turn, so no follow-up start
      message.

    - Lead pack version bumped 0.2.0 → 0.3.0.

    Lane K deviations:

    - Bumped personas/roles/lead/.plugin/plugin.json to 0.3.0. Not named in the
      brief, but inside the owned path and the pack's skill set changed;
      nothing in desktop/src-tauri or crates pins the old version (grep for
      "0.2.0" hits only crates/buzz-relay/CHANGELOG.md).

    - Skipped the setup's `pnpm install --frozen-lockfile` — this lane touches
      no JS/TS and runs no desktop gate. Rust/just gates were run from the
      activated hermit env as instructed.

    - No red-before-green test: the lane is tier-0/1 pack prose with no runtime
      behaviour. The mechanical check (pack validate/inspect resolving the new
      skill) was run before and after; before the persona frontmatter edit the
      pack still printed Valid with the skill dir unreferenced, which is why
      the frontmatter entry — not the file's existence — is what makes the
      skill reach a seat.

    Lane K residuals:

    - `bee sessions hire` did not exist when this lane was written — the skill
      documents the contract ahead of the CLI/host/relay lanes (lane C landed
      it in the same batch). If any lane's final flag names, refusal wording,
      or exit codes drift from the contract, this SKILL.md needs the same edit
      and no test will catch the drift.

    - Verbatim copy is asserted nowhere. The skill quotes 'this relay does not
      accept hire requests yet' and 'hire refused: <code> — <reason>'; nothing
      pins those strings to the CLI's actual output. A shared-constant or a doc
      test is a follow-up for whichever lane owns the CLI strings.

    - Not exercised live: no hire was published, refused, or seated from this
      pack. Evidence is pack validation and inspection only.

    - personas/roles/lead/skills/beekeeper-project/SKILL.md was left alone
      (item 80f's ledger-digest problem is another lane's), so a lead's context
      budget is unchanged by this lane apart from the new 113-line skill.

    Gate, run on the integration branch after all three lanes and the e2e fix
    (all green):

    - (1) `cargo test -p buzz-core -p buzz-sdk -p buzz-cli -p
      buzz-session-provider -p buzz-persona -p buzz-acp --lib` — 394 passed, 0
      failed. (2) `cargo test -p buzz-relay --lib` — 965 passed, 0 failed, 53
      ignored, 1 filtered (skip=demo_join_forwarded_arm_round_trips_echo). (3)
      `cargo clippy --workspace --all-targets -- -D warnings` clean. (4) `cargo
      fmt --check` clean. (5) desktop `pnpm typecheck` clean + `pnpm test`
      6,583 passed / 0 failed. (6) `cargo test --manifest-path
      desktop/src-tauri/Cargo.toml` — 2,748 passed, 0 failed, 18 ignored, plus
      csp (7), rodio (3), main (0) all ok. (7) `pnpm check:px-text` clean. (8)
      `just file-size-check` — 9/9 node tests plus desktop/web/mobile size
      scripts ok. (9) `just test` with docker Postgres+Redis — 12/12 suites
      passed, "All tests passed!". (10) port 4173 killed, `pnpm build:e2e` +
      `playwright crew-front-door.spec.ts --project=smoke` — 8 passed.

    What has to happen next, in order:

    - **This batch changes the relay** (`session.hire` on kind 44221), so hive
      must redeploy on the green pipeline before any hire can be accepted.
      Until it does, the desktop and the CLI both print "this relay does not
      accept hire requests yet" — that is the WIRE RULE working, not a bug.

    - After the deploy: Brian relaunches the dev app, launches the lead alone
      from the Team tab, and Keystone hires. Note lane H's first residual —
      **the hire hook is not mounted**, so the founder's desktop will not
      answer a hire until that plumbing lands; a hire will read `unconfirmed`
      after 60 s until then. (**Closed by item 82**, 2026-08-28 evening: the
      host is mounted and two more refusal codes exist. Item 82 is the current
      state of hiring; read it after this one.)

    **Follow-up 0ce25f6e — the host is mounted:** lane H's residual above
    was not theoretical. Live, this machine, 2026-08-28 14:37:41: Keystone's
    first hire (commandId 9a2f9956…) went unanswered because the host was
    never mounted, and the lead blocked correctly with "the founder's host
    must be online". What the follow-up seat did, verbatim:

    - MOUNT SITE (exact): desktop/src/app/AppShell.tsx:932 — `{!isHuddleRoom
      ? <CodingSessionHireHost /> : null}`, immediately after
      `<NewCodingSessionDialogHost />` (:931), inside the community-scoped
      subtree (AppShell is the root route under `<AppReady
      key={communityKey}>`); import at AppShell.tsx:54.

    - (1) HOST:
      desktop/src/features/coding-sessions/ui/CodingSessionHireHost.tsx:41
      `CodingSessionHireHost` renders nothing and sources identity, channels
      (incl. session transports), the global catalog grouped via
      groupCodingSessionCatalog, managed agents, provider identity +
      runtimes, checkout (byChannel ?? mru[0]) and the requesting seat's
      newest command target; :148 `CodingSessionHireRunner` is the hook and
      nothing else, so a test mounts the real hook against injected effects.
      Holds no module state — nothing added to resetCommunityState().

    - (1) DEPS INJECTABLE:
      desktop/src/features/coding-sessions/hooks/useCodingSessionHire.ts:109
      `CodingSessionHireDeps` (bus, worktree, create hint, the three seat
      custody/membership steps, signer, publisher, two id minters, clock)
      with :126 DEFAULT_CODING_SESSION_HIRE_DEPS as the real thing; queries
      moved out of the hook into the host's input.
      publishSeatedCodingSessionCreate stays real, so membership→custody
      ordering is observed rather than mocked.

    - (1) JSDOM TEST:
      desktop/src/features/coding-sessions/ui/CodingSessionHireHost.test.mjs
      — 6 tests, all pass. Drives a real signed 44221 through the mounted
      runner and asserts the sequence exactly
      ['worktree','hint','membership','custody','sign:44221','publish:44221']
      plus the seated create's actor/role/`[From the lead] ` first turn; a
      refusal as a 44220 turn to the requester's target (`hire refused:
      HIRE_OFF — …`) plus the umbrella lane line and nothing cut or seated;
      HIRE_STALE; the model translation and its disclosure;
      HIRE_MODEL_NOT_OFFERED with the offered ids; and one answer per hire
      however often observed.

    - RED BEFORE GREEN (1): with CodingSessionHireHost.tsx moved aside, the
      whole new suite failed `ERR_MODULE_NOT_FOUND …
      CodingSessionHireHost.tsx` — the exact state the running desktop was
      in at 14:37. Then green 6/6. Mutation check: changing `if
      (plan.modelNotice !== null)` to `if (false && …)` in
      useCodingSessionHire.ts turned the translation test red (5 pass / 1
      fail), so the suite bites; reverted.

    - (2) MODEL IDS:
      desktop/src/features/coding-sessions/lib/codingSessionHireModel.ts:57
      resolveCodingSessionHireModel (offered → byte-for-byte;
      claude-sonnet-*/opus-*/haiku-* → the catalog's family alias, `opus`
      and `opus[1m]` kept distinct; empty catalog = 'not read', refuses
      nothing), :93 describeCodingSessionHireModelRefusal (offered ids in
      the reason), :114 codingSessionHireModelNotice (the disclosure). Wired
      at codingSessionHirePolicy.ts:218-232 (+ `modelCatalogs` on
      CodingSessionHireDecisionInput, the one typecheck error the prior seat
      left) and fed from the runtimes' allowedModels at
      useCodingSessionHire.ts:302.

    - (2) DISCLOSURE: codingSessionHireAnswer.ts:191
      codingSessionHireModelNoticeLine → published as an umbrella lane
      message beside the seated create (useCodingSessionHire.ts, after
      publishSeatedCodingSessionCreate). Deviation from the brief's 'create
      title/notice': the seat's title stays the umbrella's and the brief
      stays the brief; the substitution is said in the umbrella timeline
      instead — same surface lane H used for the refusal notice, for the
      same file-ownership reason.

    - (2) RED BEFORE GREEN: codingSessionHireModel.test.mjs failed on the
      missing module and 4 codingSessionHirePolicy tests failed with
      `modelCatalogs` unknown to the decision (prior seat, kept);
      codingSessionHireModelNoticeLine's test failed `does not provide an
      export named 'codingSessionHireModelNoticeLine'` before I added it.
      Wire side (03ddbc86, kept): HIRE_MODEL_NOT_OFFERED + HIRE_STALE at
      crates/buzz-core/src/coding_session_lifecycle_command.rs:500,503 and
      the CLI's remedy table at
      crates/buzz-cli/src/commands/sessions/crew.rs:1661.

    - (3) SKILL: personas/roles/lead/skills/hire/SKILL.md:62 new '## Model
      ids' section — ids are the provider catalog's; read them with `bee
      --format json sessions status --channel <uuid>` (the `model` of each
      live execution) or the runtime's kind:44222 catalog; a
      HIRE_MODEL_NOT_OFFERED reason is itself a catalog; omitting --model is
      always safe. Alias table included; refusal table gains
      HIRE_MODEL_NOT_OFFERED and HIRE_STALE rows with a remedy each. `cargo
      run -q -p buzz-cli -- pack validate personas/roles/lead` printed
      `Valid.` (exit 0). Honest limit: `bee` has no subcommand that prints a
      runtime's catalog, so the skill does not claim one.

    - (4) STALE: codingSessionHireAnswer.ts:48
      CODING_SESSION_HIRE_MAX_AGE_SECONDS = 15*60 and the HIRE_STALE branch
      after the authority check (a stranger's stale hire is still ignored,
      not refused); codingSessionHireSeat.ts:242 sorts the unanswered
      backlog newest first (createdAt desc, stable; no createdAt keeps
      observed order). Red first: the newest-first test returned
      ['csl-old','csl-new','csl-mid'] before the sort existed; the staleness
      tests (prior seat, kept) returned `seat` for a 16-minute-old hire.

    - EXTRA FINDING, fixed: the mount had to be gated to the main window. A
      huddle room is a second window running the same AppShell down the same
      return path (session pop-outs return earlier at AppShell.tsx:679), so
      an ungated host would answer every hire twice — two identities, two
      worktrees, two processes — because the hook's dedupe set is per
      instance. Commit 56c81870.

    - GATES (all foreground, exit lines captured): desktop `pnpm typecheck`
      exit 0; `pnpm test` exit 0, 6,614 passed / 0 failed / 78 suites; `pnpm
      check:px-text` exit 0; `just file-size-check` exit 0 (AppShell.tsx
      970/1000, useCodingSessionHire.ts 456, CodingSessionHireHost.tsx 152);
      `cargo test -p buzz-core -p buzz-cli --lib` exit 0 (588 + 447 passed);
      `cargo clippy -p buzz-core -p buzz-cli --all-targets -- -D warnings`
      exit 0; `cargo fmt --all -- --check` exit 0.

    - COMMITS (all -s, no push, no rebase): 4fae757f model ids · aed8dd6a
      stale window + newest-first + disclosure line · f3d55c5c mount +
      injectable deps + jsdom suite · b0df69a0 lead skill · 56c81870
      huddle-window guard · 0ce25f6e ledger item 82. The inherited lane
      commit 03ddbc86 was kept untouched. Working tree clean.

    - LEDGER: docs/SESSION_STATE.md gains item 82 (line 4060) with the live
      14:37 finding, the two causes, the mount site, the alias rule, the
      window, the gates and five open items; item 81's 'THE HOOK IS NOT
      MOUNTED' residual and §3's lede now point at item 82 instead of
      asserting something false.

    - NOT DONE / HONEST LIMITS: (a) no live exercise — nothing here has met
      a relay, so the mounted host has never actually seated a hire and the
      huddle guard is reasoning about return paths, not an observed double
      seat; (b) the catalog checked is the runtime table's allowedModels,
      not the live `coding_session_provider_models` probe, so the two can
      disagree; (c) the CLI still reports `unconfirmed` at 60 s for a hire
      the host may later refuse HIRE_STALE; (d) I did not run `git fetch` —
      it needs NIP-98 credentials and would hang this non-interactive shell
      — but the local `origin/main` is afeff218, which is exactly this
      branch's base, so no rebase was needed.

82. **The hire that nothing answered — mounting the host, and what a model id
    is (built 2026-08-28 evening on `crew/front-door`, commits `4fae757f`,
    `aed8dd6a`, `f3d55c5c`, `56c81870`, `b0df69a0`, on top of `03ddbc86`).**

    Live finding, this machine, 14:37:41: Keystone (lead, session HiringTest,
    channel 20e2e7f0-e58f-40fd-af0d-3f8323e1de0b) published a valid
    `session.hire` — commandId 9a2f9956…, role builder, model
    `claude-sonnet-5` — and nothing answered inside 60 s. Two separate causes,
    both now fixed:

    - **Nothing was listening.** `useCodingSessionHire` was written,
      typechecked and unit-covered, and no component rendered it (item 81, lane
      H's first residual, in capitals). A running desktop therefore never
      subscribed to the 44221 stream. This is the second time a written-but-
      unmounted surface has cost a live run; the ledger entry did not prevent
      it.
    - **The model id was not one the runtime offers.** `claude-primary`'s
      catalog is `default, claude-fable-5[1m], haiku, opus[1m], sonnet`.
      `claude-sonnet-5` is a vendor name, not a catalog id, and nothing
      checked — so even a mounted host would have seated a create naming a
      model the runtime would have to reinterpret or reject later.

    What landed:

    - `desktop/src/features/coding-sessions/ui/CodingSessionHireHost.tsx`
      (new): a component that renders nothing and answers hires for as long as
      the app is open. Mounted from `desktop/src/app/AppShell.tsx:932`, beside
      `NewCodingSessionDialogHost`, inside the community-scoped subtree (so a
      community switch remounts it) and **only in the main window** —
      `{!isHuddleRoom ? … : null}`, because a huddle room is a second window
      running the same shell down the same return path, and two hosts would
      seat two agents for one hire. Session pop-outs return before that point
      already. It holds no module state, so `resetCommunityState()` gains
      nothing. Sourcing lives in the host (identity, channels incl.
      transports, the global catalog grouped into umbrellas, managed agents,
      provider identity + runtimes, the remembered checkout via
      `byChannel ?? mru[0]`, the requesting seat's newest command target);
      `CodingSessionHireRunner` is the hook and nothing else.

    - `useCodingSessionHire` now takes its outside world as
      `CodingSessionHireDeps` (bus, worktree, create hint, the three seat
      custody/membership steps, signer, publisher, two id minters, clock) with
      `DEFAULT_CODING_SESSION_HIRE_DEPS` as the real thing, and its queries as
      input rather than calling React Query itself.

    - `lib/codingSessionHireModel.ts` (new): offered ids used byte for byte;
      Claude vendor aliases (`claude-sonnet-*`, `claude-opus-*`,
      `claude-haiku-*`) translated onto the catalog's family alias when it
      offers one, and the translation **disclosed** in the umbrella
      (`codingSessionHireModelNoticeLine`, published beside the seated create);
      anything else refused `HIRE_MODEL_NOT_OFFERED` with the offered ids in
      the reason. An unread catalog (empty list) refuses nothing — that is
      "not read", not "offers nothing". `opus` and `opus[1m]` stay distinct
      ids.

    - Staleness: a hire older than
      `CODING_SESSION_HIRE_MAX_AGE_SECONDS` (15 min) is refused `HIRE_STALE`
      ("this hire request is older than the host's window; hire again") —
      checked after authority, so a stranger's stale hire is still ignored in
      silence. `selectUnansweredCodingSessionHires` now returns the backlog
      newest first (the seat ceiling is finite, so the last seat should go to
      the request the lead is actually waiting on).

    - Wire + CLI (`03ddbc86`): `HIRE_MODEL_NOT_OFFERED` and `HIRE_STALE` joined
      `HIRE_REFUSAL_CODES` in buzz-core, and `bee sessions hire` prints a
      remedy per known code after the host's own sentence (an unknown code
      prints the host's words alone).

    - Lead pack (`b0df69a0`): `personas/roles/lead/skills/hire/SKILL.md` gains
      a **Model ids** section — ids come from the provider catalog; read them
      with `bee --format json sessions status --channel <uuid>` (what live
      seats run) or the runtime's kind:44222 catalog; a
      `HIRE_MODEL_NOT_OFFERED` reason is itself a catalog; omitting `--model`
      is always safe — plus the alias table and the two new refusal rows.
      `bee pack validate personas/roles/lead` prints `Valid.`

    Red before green: `codingSessionHireModel`'s suite failed on the missing
    module and four policy tests failed with `modelCatalogs` unknown to the
    decision; the staleness tests returned `seat` for a 16-minute-old hire; the
    ordering test returned observed order; `codingSessionHireModelNoticeLine`
    did not exist; and the whole new jsdom suite
    (`ui/CodingSessionHireHost.test.mjs`, 6 tests) failed
    `ERR_MODULE_NOT_FOUND` on the host component — the exact state the running
    desktop was in. Mutating the disclosure branch to `false &&` turned the
    translation test red again, so the suite bites.

    Gates (this branch): desktop `pnpm typecheck` clean, `pnpm test` 6,614
    passed / 0 failed, `pnpm check:px-text` clean, `just file-size-check` ok;
    `cargo test -p buzz-core -p buzz-cli --lib` 588 + 447 passed; `cargo clippy
    -p buzz-core -p buzz-cli --all-targets -- -D warnings` clean; `cargo fmt
    --all -- --check` clean.

    Open after this:

    - **No live exercise yet.** Nothing on this branch has been run against a
      relay: no hire has been seated by the mounted host, and the huddle-window
      guard is reasoning about the shell's return paths, not an observed double
      seat. The next live run is the evidence.
    - The model catalog the host checks against is the runtime table's
      `allowedModels` (`getCodingSessionProviderRuntimes`), not the live
      `coding_session_provider_models` probe. Where the two differ, a model the
      probe would offer can be refused; the refusal names what this host
      believes is offered, which is at least a fact about this host.
    - The translation is disclosed as a session-lane message, not as a
      lifecycle system row — the same deviation lane H recorded for the refusal
      notice, for the same file-ownership reason.
    - `bee` has no subcommand that prints a runtime's catalog, so the skill
      points at `sessions status` and at the refusal's own list. A
      `bee sessions models` would be better.
    - The stale window is host-side only: the CLI still waits 60 s and reports
      `unconfirmed`, so a lead sees `unconfirmed` for a hire the host will
      later refuse `HIRE_STALE` if it comes back inside 15 minutes.

83 (2026-08-28 17:3x). **First hire from inside Beekeeper (session HiringTest,
    channel 20e2e7f0…).** The chain worked: Keystone's `claude-sonnet-5` hire
    refused HIRE_MODEL_NOT_OFFERED in the same second; its re-hire without a
    model seated Builder (identity default opus[1m]) at 17:07:00 with the
    brief as the first turn; the builder worked 1,009 s and committed 7e8be279
    as `Builder <1ddd35c6…@agents.beekeeper>` in hiringtest-builder-1.
    **Break:** the builder's report (`bee sessions send --to lead`) was
    refused by the relay — "only a session founder or a granted operator may
    steer" — because the hire host seated the identity but never granted it
    (the crew launcher grants after the created receipt; the hire path has no
    grant step). Keystone noticed the commit on disk with no report and
    re-addressed the builder; it could not have succeeded. Founder granted the
    builder collaborator by hand at 17:3x to unblock. Fix: the hire host
    publishes grant-operator for the seated identity after the created
    receipt, exactly as codingSessionCrewLaunch does, and the CLI's hire wait
    reports "seated, not yet granted" until the 44228 lands.

    **Fixed cd482f7e:**

    - (1) Hire host grants the seat.
      desktop/src/features/coding-sessions/hooks/useCodingSessionHire.ts:155
      adds `awaitSeatReceipt` and :161 `grantOperator` to
      CodingSessionHireDeps, defaulted at :185-:190 to the launcher's own
      `awaitCodingSessionCreateReceipt` (same 120 s
      CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS, no override) and
      `publishCodingSessionAuthorityTransition({type:"grant-operator"})` — the
      same roster/seq logic useCodingSessionCrewLaunch.ts:177 uses.

    - (1) The sequence: useCodingSessionHire.ts:447 calls `grantSeat(plan,
      hireDeps)` after the seated create, implemented at :483 — receipt first,
      then grant for `plan.actor` (the seat's own actor, not the requesting
      lead), for the launcher's reason: a grant on a create the provider
      refused is authority over a seat that does not exist. The
      model-substitution notice still publishes before the wait, so a 120 s
      receipt timeout cannot delay that disclosure.

    - (1) A failed grant is never silent. useCodingSessionHire.ts:450
      publishes both disclosures via a new shared helper
      `discloseCodingSessionHire` (:512), which publishRefusal (:557) now also
      uses — the refusal path's duplicated two-place publish collapsed into
      one function. The 44220 to the requesting seat reads exactly `seated,
      but not granted: <reason> — it cannot report until granted`
      (codingSessionHireAnswer.ts:226), and the umbrella lane line is `Hired a
      <role> — <same sentence>` (codingSessionHireAnswer.ts:234). The outcome
      gained `granted: boolean`, and a seated-but-ungranted hire carries the
      reason in `detail` instead of null.

    - (1) RED BEFORE GREEN (desktop): with the tests written and the hook
      unchanged, `node --import ./test-loader.mjs --experimental-strip-types
      --test src/features/coding-sessions/ui/CodingSessionHireHost.test.mjs`
      gave 3 failures — the ordering test's deepStrictEqual diff showed `-
      'receipt'`, `- 'grant'` missing from the effect list, and both
      disclosure tests failed on `AssertionError: the lead was never told the
      seat cannot report` / `the lead was never told`. After the change: 8/8
      pass.

    - (1) Tests at
      desktop/src/features/coding-sessions/ui/CodingSessionHireHost.test.mjs:250
      (sequence is exactly
      ['worktree','hint','membership','custody','sign:44221','publish:44221','receipt','grant']
      and `host.grants` deep-equals `[{channelId, genesisRef: GENESIS_REF,
      granteePubkey: ADA_PUBKEY}]` — the seat's actor), :374 (grant failure
      produces both the 44220 to LEAD_TARGET and the kind:9 umbrella line,
      with the create still published), :403 (a receipt that never lands is
      disclosed the same way and no grant is attempted).

    - (2) CLI. crates/buzz-cli/src/commands/sessions/crew.rs:1530 adds
      `granted: bool` to `HireOutcome::Created`; :1771 `fold_hire` takes it as
      a fourth argument. The report at crew.rs:1837 appends one sentence:
      granted true → "It holds operator authority, so it can report back to
      you"; false → "It is seated, but not granted: no grant-operator named it
      within 60s, and the relay refuses an ungranted seat's report — ask the
      operator to run `bee sessions grant --role collaborator` for it. Do not
      hire again". Outcome stays `created` and exit stays 0 either way.

    - (2) The read: crew_cmds.rs:568 folds the umbrella's authority chain
      through `super::fetch_authority_state` (kind:40099 acceptance receipts,
      `grants[actor] == "collaborator"`) and only once a seat exists — a hire
      with no seat has nothing to grant, so the extra query never runs on the
      refusal path. `read_hire_answer`/`wait_for_hire` now take `genesis_ref`.
      crew_cmds.rs:801 holds `Created { granted: false, .. }` and keeps
      polling inside the same 60 s HIRE_WAIT_SECONDS window, exactly as it
      already held `Seating` — the grant lands after the create receipt, so
      reporting the moment the provider speaks would report a mute seat.

    - (2) JSON: crew_cmds.rs:757 adds top-level `granted` — the bool for
      Created, `false` for Failed/Seating (a seat exists and nothing granted
      it), and `null` for Refused/Unconfirmed, where no seat was created and
      `false` would assert a different fact.

    - (2) RED BEFORE GREEN (CLI): with the test written and crew.rs unchanged,
      `cargo test -p buzz-cli --lib` failed to build with `error[E0026]:
      variant sessions::crew::HireOutcome::Created does not have a field named
      granted` at crew_tests.rs:2196, plus E0061 arity errors on every
      fold_hire call. Test at
      crates/buzz-cli/src/commands/sessions/crew_tests.rs:2164 asserts the
      ungranted fold is `Created { granted: false }`, status `created`, exit
      0, detail containing "seated, but not granted" and "bee sessions grant
      --role collaborator", and that the granted fold says "can report back"
      and never "not granted".

    - (3) personas/roles/lead/skills/hire/SKILL.md:62 adds "Read `granted`
      before you end your turn": what true/false/null each mean, the relay's
      exact refusal sentence, the 2026-08-28 break as the reason, the `bee
      sessions grant --channel … --genesis … --pubkey … --role collaborator`
      line with a note that `genesisRef` and `seat.actor` both come from the
      hire output, that only the founder may extend the chain so it is a
      request to the person, and an explicit "do not hire again" (a second
      agent would have the same problem and two lanes would own one brief).
      `cargo run -q -p buzz-cli -- pack validate personas/roles/lead` →
      "Valid."

    - Not done / worth knowing: nothing in the desktop UI consumes
      `CodingSessionHireOutcome` — the hook's outcomes array has no reader
      (grep found only CodingSessionHireHost.tsx mounting the hook), so the
      `granted` field there is currently for tests and future screens; the
      user-visible disclosure is the 44220 turn and the umbrella lane line,
      which is where the lead and the person actually look. No live relay run
      was performed — this is unit-level evidence only, and the next real hire
      is what confirms the grant lands end-to-end.

    Gates on this branch: `cargo test -p buzz-core -p buzz-cli -p
    buzz-session-provider -p buzz-persona -p buzz-acp --lib` 394 passed / 0
    failed; `cargo clippy` workspace all-targets `-D warnings` 0 warnings;
    `cargo fmt --check` clean; desktop `pnpm typecheck` clean and `pnpm test`
    6,616 passed / 0 failed; `cargo test` (desktop/src-tauri) 2,748 passed / 0
    failed / 18 ignored; `pnpm check:px-text` clean; `just file-size-check` 9
    passed / 0 failed; `bee pack validate personas/roles/lead` Valid; `pnpm
    build:e2e` plus Playwright `crew-front-door.spec.ts --project=smoke` 8
    passed / 0 failed.

84. **Designer seat for the Singularity remodel (2026-08-28 18:0x).**
    Brian's mock + design doc live at /Users/brian/Downloads/singularity/
    (Singularity.png, Keystone.png, beekeeper-singularity-ui-remodel.md —
    written by a design agent from Brian's description alone, no wire
    knowledge). Decision: dogfood it — hire **Banksy** (persistent designer
    identity, Codex gpt-5.6-sol) as `designer` to write the Surfaces spec
    from the mock + doc, then builder lanes. Prerequisite pack lane:
    designer gains `see-the-app` (the poker's drive skill by reference), a
    `wire-sources-for-surfaces` table (44223 status/lease → liveness; 44224
    → stages; 44240 types → dispositions; 44228 → authority; 44227 → goal;
    44225 → story; 44226 → founder; write-report fields → changes/tests,
    else "no report yet"), and a "from mock to spec" section; installer asks
    a name per identity ("Name your team"). Naming ruling: Singularity is
    the surface, sessions stay sessions.

    Banksy persona direction (Brian, 18:1x): "like Banksy the artist —
    outside the box; the artist literally made spray paint feel amazing, not
    just look good." The designer persona body carries this: the surface
    must *feel* right, not merely be correct — the stream tells the story,
    the state is felt before it is read; bold simplification over
    decoration; break a convention when the convention lies, never when it
    merely bores; and every feeling is still built on a signed fact (the
    wire table) — a beautiful lie is the one thing Banksy never ships.
    Renamed on disk 18:1x (identity efefd4e5…, persona card, default Codex ·
    gpt-5.6-sol); the relay profile republishes on the next Install team
    roles refresh or a dialog rename.

    **Shipped `d2706451`:**

    - **Lane B — designer pack becomes Banksy** (`be381f7d`):

      - designer.persona.md rewritten (89 lines, under the 90 cap): keeps
        name: designer / role: designer, carries Brian's Banksy direction
        verbatim in spirit — the artist made spray paint feel amazing, not
        just look good; correct is the floor, the stream tells the story,
        state is felt before it is read, bold simplification over
        decoration, break a convention when it lies and never when it merely
        bores, every feeling rests on a signed fact and a beautiful lie is
        the one thing this seat never ships. The existing 'What you never
        do' is kept intact (no feature code/tests/migrations, no invented
        surfaces, no control that claims what it does not enforce, cite
        file:line) plus a new line: never specify a state you have not seen.
        Frontmatter skills list now names all three skills.
      - NEW skills/see-the-app/SKILL.md (67 lines): references
        personas/roles/poker/skills/drive-and-report by path rather than
        copying it (stated reason: a copy drifts and the poker's is the one
        that gets fixed), then names what a designer uses differently — a
        mock bridge is legitimate for cataloguing states where a poker needs
        the real build; reach every state you intend to specify (or write
        'not reached: <state> — <why>'); crop to the subject; hash the set
        before use because unscoped captures come out byte-identical; you
        specify rather than report findings, and an honesty bug found while
        driving goes to the lead as an anomaly. Mocks are read as images and
        cited file + region + element (example: Singularity.png ->
        participant status bar -> Keystone row); an element seen and not
        carried is listed as deliberately dropped.
      - NEW skills/wire-sources-for-surfaces/SKILL.md (62 lines, under the
        120 cap): a 12-row table of UI fact -> signed source ->
        empty/unknown copy, grounded in verified code. Rows: liveness (44223
        status + CodingSessionLeaseState live/released -> 'live / quiet 3d /
        released / no provider answering'); turn stages (44224
        turn_queued/turn_started/turn_degraded/turn_dropped/turn_refused/interrupt_delivered
        -> 'no receipt yet'); dispositions (44240 pu-type
        plan|milestone|note|handoff|blocker, rendered as a claim by its
        author); authority (44228 grant-operator -> 'seated, not yet
        granted'); goal (44227); founder (44226 signer, identity is the
        genesis event id not the csg-session tag); story (44225 -> 'no turn
        observed'); seats (44223 agentRef + role, display name from the
        kind-0 profile, never a pubkey); hires (44221 session.hire +
        HIRE_MODEL_NOT_OFFERED / HIRE_STALE refusals, and a relay predating
        session.hire says so rather than blaming the provider);
        changes/tests (no kind today — the builder write-report fields
        arrive as prose, so 'no report yet' is the only honest value, never
        scrape a count out of prose); plan (no artifact — the lead brief's
        'Acceptance:' line is prose, so 'no plan published'). Carries the
        rule verbatim: a panel that cannot name its row shows the unknown
        copy, never a number; plus absence/unknown/empty are three states
        and claims are never painted as facts.
      - specify-surfaces/SKILL.md gains '## 5. From a mock to a spec' (Hand
        off renumbered to 6; file now 110 lines): the per-element walk
        writes five lines each — cite the mock (file -> region -> element),
        the surface it replaces (file:line or 'new surface'), the wire row
        it reads, verbatim copy including empty/unknown, and the poker's
        walk; dropped elements are written as dropped; a mock label loses to
        the wire (mock 'Idle' over an unreachable provider becomes 'No
        provider answering', recorded as an override); density and lifecycle
        are split as separate axes. Includes the naming ruling: Singularity
        is the surface, sessions (sessionRef, umbrella session, execution,
        generation) stay sessions on the wire, in the CLI and in code, and
        no copy may assume a lead exists.
      - poker drive-and-report/SKILL.md: one blockquote note added under the
        title saying the designer's see-the-app references this procedure
        rather than copying it, so keep it general (32 lines, the permitted
        one-line note).

      Open from this lane:

      - personas/roles/designer/.plugin/plugin.json still carries id
        `com.beekeeper.crew.designer` and name 'Team Designer'. The id is
        'crew'-flavoured but is an install key, not user-facing copy;
        renaming it risks orphaning installed packs, so I left it. Someone
        owning the installer should decide whether pack ids migrate.
      - The wire table's 'changes/tests' and 'plan' rows name two facts with
        no signed source today. Those are the next event kinds somebody has
        to add if a Singularity panel is ever to show a test count or an
        accepted plan; the skill states them as sign-off items rather than
        resolving them.
      - Not exercised: no designer seat was actually run against this pack,
        so the skills are validated as a pack (pack validate Valid.) but not
        yet proven in a live hire. Item 84's Banksy hire is the real test.

    - **Lane N — installer asks a name per identity and republishes
      the profiles it renames** (`abedb46d`, `d2706451`):

      - (1) "Name the lead" is gone; the dialog now shows
        INSTALL_CREW_ROLES_TEAM_NAMES_LABEL = "Name your team" with one text
        field per role pack the folder scan found, in the scan's order (lead
        first, then the roster, then the unseated roles).
      - (1) The folder pick now carries a read-only scan:
        pick_crew_role_packs_directory returns PickedCrewRolePacks {
        directory, packs, skipped }, so the fields appear the moment a
        folder is chosen. Each field defaults to the name that identity
        already carries on this computer (Keystone, not lead) when a pack is
        already installed, otherwise the pack's display name — so installing
        without touching a field renames nobody.
      - (1) The install request carries names: Record<role, name>;
        install_role_packs takes &HashMap<String,String> instead of
        lead_name: Option<&str>. A name over an already-installed identity
        renames it in place — managed-agent record and persona card — and
        mints nothing (D11 "minted once"). Rust reds:
        every_named_role_is_minted_under_the_name_the_operator_gave,
        a_new_name_renames_the_installed_identity_in_place_and_mints_nothing,
        the_name_fields_come_off_the_scan_with_the_lead_first. TS reds:
        field list renders from the scan and the submit carries the full
        role→name map (installCrewRolesForm.test.mjs).
      - (2) Verified role_profile_publishes already covers every installed
        identity, not only the lead
        (desktop/src-tauri/src/managed_agents/crew_roles.rs — it maps over
        install.installed unconditionally, and commands/crew_roles.rs
        publishes each). No extension needed; the gap was that no seat other
        than the lead could be renamed. New red test
        renaming_the_designer_owes_a_profile_publish_with_the_new_name pins
        designer→Banksy owing a kind:0 publish with previous_name
        Some("designer").
      - (3) A renamed row reads "Designer — Banksy (renamed; profile
        republished)" via the new crewRoleResultLine +
        INSTALL_CREW_ROLES_RENAMED_NOTE; the rename note replaces the
        refresh note rather than stacking on it. InstalledCrewRole gained a
        renamed flag distinct from refreshed. All copy says "team".
      - Honesty fix found on the way:
        InstallCrewRolePacksResponse.profileSyncError existed in Rust but
        was missing from the TS type, so a run whose kind:0 publishes failed
        showed the operator nothing while rows still said "profile
        republished". It is now in the type, rendered in the dialog
        (data-testid install-crew-roles-profile-sync-error), and a renamed
        row on such a run reads "renamed here; the relay may still know it
        by the old name" instead.
      - A folder that scans to zero packs now says so before anything is
        installed and disables Install, rather than only after a write.

      Deviations:

      - No new Tauri command. Registering one requires
        desktop/src-tauri/src/handlers.rs, which this lane does not own, so
        the scan travels back on the already-registered
        pick_crew_role_packs_directory (its return type changed from
        Option<String> to Option<PickedCrewRolePacks>). Scanning is
        read-only, so doing it at pick time writes nothing.
      - crew_roles_tests.rs was already at the 1000-line ceiling, so the
        naming tests live in a sibling module
        desktop/src-tauri/src/managed_agents/crew_roles_naming_tests.rs
        (declared #[path] mod naming inside the existing tests module). The
        limit was not bumped.

      Open from this lane:

      - Not run in the live app — evidence is unit/DOM tests, typecheck,
        clippy and the file-size gate only. No screenshot of the new field
        list.
      - Ledger item 85 (default the folder to <checkout>/personas/roles) is
        untouched — different lane, and it would land in the same dialog's
        folder row.
      - The success toast in AgentsView.tsx (crewRolesInstalledToast) still
        says "Installed N team roles into <team>: roles" and says nothing
        about renames. AgentsView.tsx is not owned by this lane; the toast
        helper is, if a follow-up wants it to mention renamed identities.
      - profileSyncError is one sentence for the whole run, not one per
        identity, so when it is set every renamed row hedges. Making the
        hedge per-identity needs the backend to return the failures keyed by
        pubkey.
      - Two packs declaring the same role would share one map entry and one
        field; the second identity would install as "<name> 2" via
        mint_agent_name. Not exercised by any test — the repo's packs are
        one role each.

    Gates on this branch: `cargo test -p buzz-cli -p buzz-persona -p
    buzz-core --lib` 589 + 447 + 157 = 1,193 passed / 0 failed; `cargo
    clippy --workspace --all-targets -D warnings` 0 warnings; `cargo fmt
    --all --check` clean; desktop `pnpm typecheck` clean and `pnpm test`
    6,635 passed / 0 failed across 80 suites; `cargo test`
    (desktop/src-tauri) 2,752 + 7 + 3 = 2,762 passed / 0 failed / 18
    ignored; `pnpm check:px-text` clean; `just file-size-check` 9/9 node
    subtests plus the desktop/web/mobile size checks clean; `pack validate`
    for lead, architect, builder, runner, verifier, poker and designer — all
    seven "Valid."; `pnpm build:e2e` plus Playwright
    `crew-front-door.spec.ts --project=smoke` 8 passed / 0 failed in 17.8 s.

85 (draft, 2026-08-28 19:0x). **Install team roles has no default folder**
    — a new operator (Andy) must know to pick `<checkout>/personas/roles`.
    Fix: default the field to the project's checkout dir +
    `/personas/roles` (project settings now record the checkout dir —
    Andy's 75f9fc8f), show it as the chosen path with "the project's role
    packs" as the label, keep *Choose folder…* for the exception; later,
    D12's project-ref fetch replaces the picker.

    **Fixed ae2b3bbe:**

    - Worktree setup:
      /Users/brian/Projects/beekeeper/beekeeper.worktrees/fd-int, branch
      crew/front-door, clean, `git fetch origin` + `git merge --ff-only
      refs/remotes/origin/main` -> 'Already up to date.' (base 32f2e8e4).

    - New Rust policy (ledger 85):
      desktop/src-tauri/src/managed_agents/crew_roles.rs:325
      `project_role_packs_dir(checkout) = checkout/personas/roles`, and
      :353 `scan_project_role_packs(checkout, agents) ->
      ProjectRolePacksScan { directory, exists, packs, skipped }`. A
      missing folder returns Ok with `exists: false` (an ordinary state,
      not a fault); Err is kept for a folder that is there and unreadable.
      `packs` is literally `role_name_choices(&scan_role_packs(dir),
      agents)` so the pre-chosen path and the picker path can never
      disagree.

    - New command: desktop/src-tauri/src/commands/crew_roles.rs:111
      `scan_project_role_packs_directory(checkoutDir)` (read-only,
      spawn_blocking, loads managed agents so name fields default to the
      installed identity's name). Registered at
      desktop/src-tauri/src/handlers.rs:250. This is the one new
      command/param the task asked me to report.

    - TS wrapper: desktop/src/shared/api/tauriTeams.ts:315
      `ProjectRolePacksScan` + `scanProjectRolePacks(checkoutDir)`.

    - Copy: desktop/src/features/agents/ui/installCrewRolesCopy.ts:23
      `INSTALL_CREW_ROLES_PROJECT_FOLDER_LABEL = "The project's role
      packs"`; :34 `INSTALL_CREW_ROLES_NO_CHECKOUT` verbatim "This project
      has no checkout directory yet — set one in Project settings, or
      choose a folder"; :46 `crewRolesProjectFolderNote(scan)`
      distinguishes absent ("<path> is not there, so this project has no
      role packs to install — choose a folder instead.") from empty
      ("<path> holds no role packs — choose a folder instead.").

    - Dialog: desktop/src/features/agents/ui/InstallCrewRolesDialog.tsx:63
      optional `project: { address }` prop; :101 effect reads
      `getCodingSessionWorkdirState().byProject[address].path` (Andy's
      per-project checkout store, 75f9fc8f), scans it, and either applies
      the scan (`isProjectFolder` true, label + name fields rendered,
      submit enabled) or sets a note. `operatorChose` ref (:99) means a
      slow lookup can never overwrite a folder the operator picked;
      `choose()` clears the label and note (:167).

    - Wiring: desktop/src/features/agents/ui/AgentsView.tsx:59 resolves
      the active project with the app's own resolution via the new
      `useActiveProjectContainer`
      (desktop/src/features/projects-container/useActiveProjectTint.ts:47,
      which `useActiveProjectTint` now delegates to), and passes `project`
      at AgentsView:314.

    - RED BEFORE GREEN (TS): 4 new tests in
      desktop/src/features/agents/ui/installCrewRolesForm.test.mjs:340+
      failed against the old dialog — 'a project whose checkout holds role
      packs opens on that folder, already chosen', 'a project with no
      checkout directory says so, and the picker is untouched', 'a
      checkout with no personas/roles folder names the folder it looked
      for', 'a personas/roles folder holding no packs is reported as
      empty, not as missing' (run: 13 tests, 9 pass, 4 fail — 'Unable to
      find an element by:
      [data-testid="install-crew-roles-project-note"]'). After the change:
      13 pass, 0 fail. Two more tests pin the unchanged paths ('outside a
      project ... no lookup, no note' asserts zero
      `scan_project_role_packs_directory`/`get_coding_session_workdir_state`
      calls; 'choosing another folder over the project's default drops the
      project label').

    - RED BEFORE GREEN (Rust): 4 new tests in
      desktop/src-tauri/src/managed_agents/crew_roles_tests.rs:790+ failed
      to compile — 'error[E0425]: cannot find function
      `project_role_packs_dir` in this scope' and '...
      `scan_project_role_packs` ...', 7 errors, 'could not compile
      beekeeper-desktop (lib test)'. After the change: `cargo test --lib
      crew_roles` = 27 passed, 0 failed.

    - Gates, each with its exit line: desktop `pnpm typecheck` EXIT 0;
      `pnpm test` EXIT 0 (6641 tests, 6641 pass, 0 fail); `pnpm
      check:px-text` EXIT 0; `cargo test --manifest-path
      desktop/src-tauri/Cargo.toml --lib crew_roles` EXIT 0; `cargo clippy
      --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D
      warnings` EXIT 0; `cargo fmt -- --check` EXIT 0; `just
      file-size-check` EXIT 0 (largest touched file 920 lines,
      crew_roles.rs).

    - E2E: killed port 4173, `pnpm build:e2e` EXIT 0, `npx playwright test
      tests/e2e/crew-front-door.spec.ts --project=smoke` EXIT 0 — 8
      passed. Test 01 needed no update: the dashboard route names no
      project, so the installer it drives is byte-for-byte the surface it
      was, and the existing screenshots keep their distinct hashes.

    - FINDING (reachability, honest): the pre-chosen folder cannot appear
      in the app today. The only entry point is TeamsSection's 'Install
      team roles…' inside AgentsView, which is a Dashboard tab at
      `/?tab=agents`; `resolveActiveProjectId`
      (useActiveProjectTint.ts:20) needs either a `/projects/<id>` route
      or an active channel's `projectRef`, and the dashboard has neither
      (deriveAppSurface in AppShell.helpers.ts:225 yields
      `selectedChannelId: null` there). So `project` is null at that call
      site and the dialog behaves exactly as before. Making it live needs
      a product decision I did not take on my own: either an 'Install team
      roles…' entry on a project surface, or giving the Agents tab a
      project. Everything below that entry point is built, tested and
      ready for it.

    - Not done on purpose: no push, no ledger edit (docs/SESSION_STATE.md
      item 85 is the lead's to write), no new entry point invented.

    - Reachability closed by item 86.

86 (2026-08-28 20:0x). **The Agents tab names its project, so Install
    team roles opens on the project's role packs** — item 85 built the
    pre-chosen folder but nothing could reach it, because the Agents tab
    names no project. Fixed on `crew/front-door` (`49ef9930`,
    `8abdd599`): the tab resolves a project itself, says which one it
    picked, and lets the operator change it.

    - WHERE THE FALLBACK PROJECT COMES FROM — I searched for a durable last-opened/active project record and there is none. What exists: buzz.projects.projectScope (a *filter* on the Projects view, defaults to "all" — desktop/src/features/projects/lib/projectsViewHelpers.ts:113), buzz.projects.collapsed.v1, buzz.projects.sessionFilter.v1, buzz.projects.defaultAgent.v1, buzz.projects.containers.v1 (a first-paint cache). None records a visit. No route/history persistence either. So the recency signal is the coding-session workdir store's per-project updatedAt (ISO-8601 from now_iso(), desktop/src-tauri/src/coding_sessions/workdir_store.rs:64) — durable, host-local, and the same fact that decides whether a project has a personas/roles folder at all. Behind it: the first project the viewer can see.
    - RESOLUTION ORDER — desktop/src/features/agents/lib/rolePacksProject.ts:73 resolveRolePacksProject: route -> operator's own pick -> newest recorded checkout -> only/first membership -> none. Sources enumerated at :22 so the caller can say why. A chosenId naming a deleted project falls through instead of blanking. A checkout recorded for a project the viewer cannot see is ignored.
    - WIRING — hook desktop/src/features/agents/ui/useRolePacksProject.ts:40 (workdir query at :51, enabled only when there is more than one project, so a single-project machine reads nothing). AgentsView consumes it at desktop/src/features/agents/ui/AgentsView.tsx:63, passes the selector at :306 and the named project at :330. TeamsSection renders the slot under its section header, above the cards, at desktop/src/features/agents/ui/TeamsSection.tsx:74.
    - SELECTOR — desktop/src/features/agents/ui/RolePacksProjectSelector.tsx:32, "Role packs for: <name> ▾", reusing ProjectsListScopeDropdown rather than a second dropdown. Renders nothing with 0 or 1 project (the single project is still named on the dialog's label). Copy at desktop/src/features/agents/ui/installCrewRolesCopy.ts:58.
    - DIALOG LABEL — desktop/src/features/agents/ui/installCrewRolesCopy.ts:43 installCrewRolesProjectFolderLabel -> "The project's role packs — <name>", rendered at desktop/src/features/agents/ui/InstallCrewRolesDialog.tsx:258. Deliberate change of scope from the brief: the label now renders whenever a project was *resolved*, not only when its folder turned out usable — otherwise a resolved project whose checkout is missing was never named anywhere, which is the silent guess the task forbids. A folder the operator picks themselves retires the label.
    - STALE-STATE FIX — InstallCrewRolesDialog.tsx:137: switching project under an open dialog left the previous project's folder, label and note on screen until the new scan landed. Now cleared before the lookup runs.
    - HONESTY BUG FOUND IN THE LIVE E2E RUN — a failure reading the workdir store was reported as "That folder could not be read", blaming a folder the dialog had never opened. Split into two reads with their own sentences: crewRolesCheckoutLookupFailed (installCrewRolesCopy.ts:232) used at InstallCrewRolesDialog.tsx:154; the folder scan keeps crewRolesUnreadableFolder. Two tests pin both halves.
    - CLICK-SWALLOWING BUG FOUND IN THE LIVE E2E RUN (commit 8abdd599) — after choosing a project, the next click on the page was retried until Playwright timed out at 30s ("element was detached from the DOM, retrying"). Cause: Radix's modal dropdown holds pointer-events:none on the body for a beat after selection. Every other menu in the agents library already passes modal={false}; ProjectsListScopeDropdown now takes it as a prop (desktop/src/features/projects/ui/ProjectsListScopeDropdown.tsx:38, default true so ProjectsView is unchanged) and the selector sets it (RolePacksProjectSelector.tsx:47). Verified: same probe passes in 1.9s after the fix, and DOM churn measured at 3 mutations in 2s (no render loop).
    - RED BEFORE GREEN, ROUND 1 — run of the three test files before any implementation: EXIT 1. 8 resolver tests and 4 selector tests failed ERR_MODULE_NOT_FOUND (rolePacksProject.ts, RolePacksProjectSelector.tsx absent). 3 form tests failed on real diffs: actual "The project's role packs" vs expected "The project's role packs — Beekeeper"; actual "The project's role packs" vs "The project's role packs — Attic"; actual '/checkout/personas/roles' vs expected '' (stale folder after switching). After implementing: 27 passed / 0 failed, EXIT 0.
    - RED BEFORE GREEN, ROUND 2 (the two defects the live run exposed) — 2 failures before the fix: the folder label was absent when a project resolved but its checkout was missing, and the note read 'That folder could not be read: Unsupported mocked Tauri command'. After: installCrewRolesForm.test.mjs 17 passed / 0 failed, EXIT 0.
    - TESTS ADDED — 8 in desktop/src/features/agents/lib/rolePacksProject.test.mjs (a: one project and no route resolves it; c: none resolves nothing; plus route/chosen/recency/stale-choice/unseen-checkout ordering). 4 in desktop/src/features/agents/ui/rolePacksProjectSelector.test.mjs (b: renders with two projects and names the resolved one; opening the Radix menu and choosing reports the switch once; nothing rendered with one or none). 4 in installCrewRolesForm.test.mjs (switching re-scans the new checkout and drops the old folder; switching to a project with no checkout clears the folder it replaced; lookup failure not blamed on a folder; a real folder failure still is).
    - E2E TEST 01 WAS UPDATED, because the dashboard fixture DOES now resolve a project — a mock project named "buzz". Added a get_coding_session_workdir_state answer in front of the bridge (tests/e2e/crew-front-door.spec.ts:387; the shared bridge does not answer that real command, which is what produced the false folder-blame), and assertions at :650-662 that the folder label matches /^The project's role packs — .+/, that the no-checkout note is verbatim, that no selector is offered with one project, and that picking a folder retires the label. Screenshot 02-install-idle.png now shows the named project and the honest note; all 16 PNG hashes unique (0 duplicates).
    - NEW PERMANENT SPEC — desktop/tests/e2e/role-packs-project.spec.ts (registered at desktop/playwright.config.ts:35). Seeds two extra kind:30621 heads via __BUZZ_E2E_EXTRA_PROJECT_EVENTS__, then drives: selector visible with three projects ("Role packs for: Attic"), installer label "The project's role packs — Attic", switch to another project, selector re-labels, installer re-opens as "The project's role packs — buzz". Ledger 85's first pass shipped an unreachable path with every unit test green; this is the spec that would have caught it.
    - GATES, each with its exit line: desktop pnpm typecheck EXIT 0 · pnpm test EXIT 0 (6657 tests, 6657 pass, 0 fail, 80 suites; was 6641 before this change) · pnpm check:px-text EXIT 0 · just file-size-check EXIT 0 (9/9 node subtests plus desktop/web/mobile checks; largest touched file 688 lines, AgentsView.tsx) · desktop biome check . EXIT 0 (2 warnings, 6 infos — all pre-existing, confirmed by stashing and re-running on the clean base) · killed port 4173, pnpm build:e2e EXIT 0 · npx playwright test crew-front-door.spec.ts role-packs-project.spec.ts --project=smoke EXIT 0, 9 passed in 18.8s.
    - NOT DONE ON PURPOSE — no push, no Rust touched, no ledger edit (docs/SESSION_STATE.md item 85's follow-up is the lead's to write). No new persistence invented for the operator's selector choice: it lasts for the surface's lifetime, not across launches. If a durable choice is wanted, the natural home is a localStorage key beside buzz.projects.projectScope, keyed by pubkey+relay.
    - TOOLING GOTCHA worth recording — node:test OOM-kills (exit 137, ~77s, no diagnostic) when an assertion compares a jsdom element to null, e.g. assert.equal(screen.queryByTestId(...), null) once the element exists. It looks exactly like an infinite render loop; a render counter proved there was none (fewer than 200 renders) and the V8 stack showed NewRawTwoByteString. The fix is to assert on textContent instead.

    Gate lines from the landing run: Gate 1 desktop typecheck + `pnpm
    test` 6657 pass / 0 fail, EXIT 0 (/tmp/swat10-gate-1.log); Gate 2
    `pnpm check:px-text` EXIT 0 (/tmp/swat10-gate-2.log); Gate 3 `just
    file-size-check` 9 unit tests pass, EXIT 0 (/tmp/swat10-gate-3.log);
    Gate 4 `cargo test crew_roles` 27 pass / 0 fail, EXIT 0
    (/tmp/swat10-gate-4.log); Gate 5 Playwright
    `crew-front-door.spec.ts` smoke 8 passed, EXIT 0
    (/tmp/swat10-gate-5.log).

87 (draft, 2026-08-28 21:2x). **Team launch from the Dashboard: it worked and the person could not find it.** Wire: 21:21:18 genesis in a freshly minted channel `ca254767…` ("Beekeeper sessions"), members added, seated create for Keystone (title "UI", model opus[1m]) at 21:21:29, `created` at :31, the goal turn started at :32. But the create carries `projectRef: None` — the dialog was opened from the Dashboard, not a project, so the launcher minted an unscoped channel — and the launch closed the dialog without navigating, so the session appears nowhere the person was looking (the project's session list groups by projectRef). Fix: (a) the Team launch either carries the project (resolve it the way item 86 resolves it for the installer, and say which project on the tab) or discloses "this session will not belong to a project"; (b) after a successful launch, open the lead's session (the one-session path does); (c) Team tab model/thinking picker (item 82) — the lead landed on opus[1m], not Fable. Workaround now: sidebar → channels → the newest "Beekeeper sessions".

    Correction 21:3x: Brian launched from INSIDE the project (Projects → Bee Keeper → New session → Team), so (a) is a launcher bug, not a wrong entry point: the team launch does not carry projectRef even with a projectContext. Worse, (d) the Team tab has no worktree toggle and runs the LEAD in the checkout it names — Keystone's cwd is the hot main checkout (state.json), its skills materialized into it (untracked .agents/skills/*), while the seats it hired (Builder → ui-builder-1, Banksy → ui-designer-1) got worktrees. The launcher must default the lead to a worktree exactly as the join dialog does; the provider should also refuse a seat whose cwd is the checkout the app runs from. (e) No "Stop all" / per-seat stop the person can find — the mock's Stop all is needed now.

    **Fixed 8574bd7d:**

    - PROJECT (87a) — desktop/src/features/coding-sessions/lib/codingSessionCrewLaunch.ts:180 adds `projectRef` to CodingSessionCrewLaunchInput and passes it to publishSeatCreate at :560; ui/useCodingSessionCrewLaunch.ts:167 signs it into buildCodingSessionCreateEvent (was hardcoded `projectRef: null`) and :190 files recordPendingCodingSessionLifecycle exactly as the one-session path does, so the row appears in the project immediately. NewCodingSessionDialog.tsx:566 passes projectName/projectRef/defaultWorkdir from projectContext.
    - PROJECT disclosure — codingSessionCrewLaunch.ts:codingSessionCrewProjectNote() renders in NewCodingSessionCrewTab.tsx under data-testid="new-coding-session-crew-project": "Launching in Bee Keeper: …" with a project, "This session will not belong to a project — it lives in the channel above, not in a project's sessions." without one.
    - WORKTREE (87d) desktop — codingSessionCrewLaunch.ts gains CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP, `leadWorktreeRequest()`, a `createLeadWorktree` dep and `leadWorkdir` on the result; the worktree is cut after the family check and BEFORE the genesis, so a failure costs nothing signed. NewCodingSessionCrewTab.tsx now renders the same NewCodingSessionWorktreeField the one-session path uses, on by default, prefilled `<session-slug>-lead` via new codingSessionLeadWorktreeName() in lib/codingSessionWorktreeName.ts. The hook stages the create hint against the worktree and calls recordCodingSessionWorkdirUse(checkout) — the recent list learns the checkout, never the worktree.
    - WORKTREE (87d) provider — crates/buzz-session-provider/src/session.rs: shared roots are now typed (SharedWorkdirRoot / SharedWorkdirKind::{Operator,AppCheckout}); shared_workdir_roots() reads BUZZ_CSP_SHARED_WORKDIRS (SHARED_WORKDIRS_VAR) in addition to $HOME and the nest; seated_workdir_refusal returns SEAT_CWD_SHARED with "that is the checkout the app runs from, shared with the person driving it". Still gated on plan.actor.is_some() (lib.rs:1347), so a person's own session in their checkout is untouched.
    - WORKTREE (87d) host — desktop/src-tauri/src/session_provider/env.rs adds ProviderEnvInputs.app_checkout, exports SHARED_WORKDIRS_VAR, and adds resolve_app_checkout(dir, home) (walks up for `.git`, refuses to call a home directory a checkout) + app_checkout_dir(); supervisor.rs:663 fills it per spawn. LIMITATION, reported exactly: this resolves only when the desktop process runs inside a repo — i.e. `just dev` / `cargo tauri dev`, which is the configuration the live incident happened in. A bundled .app launched from Finder has cwd `/`, resolves no checkout, and the variable is absent (deliberately absent, not empty). I did NOT wire project checkouts into the list; the provider already has BUZZ_CSP_PROJECTS_FILE and that is the natural follow-up.
    - NAVIGATE (87b) — codingSessionCrewLeadDestination() builds the lead's generationId from its own receipt with buildCodingSessionTranscriptGenerationId(channelId, providerAuthority, target) — the identical pure key useCodingSessionCatalog.ts:313 mints, so no catalog round-trip and it cannot open a different session. NewCodingSessionDialog.tsx onLaunched now calls onDone() then goCodingSession(..., {replace:true}).
    - STOP ALL (87e) — new lib/codingSessionStopAllModel.ts (buildCodingSessionStopAll) + ui/useCodingSessionStopAll.ts; CodingSessionHeader.tsx gains onStopAll/stopAllCount rendering data-testid="coding-session-stop-all" ("Stop all (3)"). Founder-only and hidden — not disabled — for anyone else. Executions that are `ended`, or that carry no target or no provider authority, are excluded from BOTH the count and the fan-out. Publishing reuses publishEndCodingSessionRequest → publishCodingSessionStop, the unanswered-provider toast, and the pending stop rows; EndCodingSessionRequest gained an optional `confirm` so the bulk dialog can retitle itself without a second dialog drifting.
    - DEVIATION 1 (honesty) — the brief's confirm copy "Sessions stay open; seats can be resumed" is false in this app: Reconnect renders for `disconnected` only (desktop/src/features/coding-sessions/ui/CodingSessionComposerSurface.tsx:362), and the single-stop confirm has said a stopped execution cannot be resumed since §2 item 42. Shipped copy: title "Stop N live seats?", body "…The session stays open — its transcript, its people, and its goal are untouched — but a stopped seat cannot be resumed: to carry the work on, add a provider to the session." A test asserts the description does not promise a resume.
    - DEVIATION 2 — "hide otherwise, with the reason in a tooltip" is self-contradictory for a hidden control. Implemented as: control absent for non-founders (no disabled button to click at), the model still returns the reason string for a caller/test that wants it, and the visible control carries the tooltip "Stop every live seat in this session. The session stays open; a stopped seat cannot be resumed."
    - RED BEFORE GREEN 1 — reverting the two lines that pass projectRef/workdir into publishSeatCreate failed 3 of the new launch tests: `actual: [ { role: 'lead', projectRef: null, workdir: null } ]` vs expected `'34550:owner:beekeeper'`, and `actual: null` vs `'/Users/b/Projects/bk/bk-ui-lead'` / `'/Users/b/Projects/bk/bk'`. Restored: 30/30 pass.
    - RED BEFORE GREEN 2 — the provider tests failed to compile against the old Vec<PathBuf> roots (E0433/E0425/E0609 on SharedWorkdirRoot, parse_shared_workdirs, `.path`); the host tests failed with E0432 (no resolve_app_checkout / SHARED_WORKDIRS_VAR) and E0063 (no field `app_checkout`). After the implementation: buzz-session-provider 396 passed, beekeeper-desktop 2758 passed.
    - RED BEFORE GREEN 3 — forcing CodingSessionHeader's `{onStopAll ? …}` to always render failed the new header test with "The input was expected to not match the regular expression /coding-session-stop-all/". Restored: 16/16 header tests pass.
    - FILE-SIZE — wiring Stop all put CodingSessionUmbrellaWorkspace.tsx at 1037 lines. Split, not raised: stop-all wiring → ui/useCodingSessionStopAll.ts (100), focus notice → ui/CodingSessionFocusedAgentNotice.tsx (56); workspace now 960. `just file-size-check` green.
    - GATES — desktop: pnpm typecheck clean, pnpm test 6675 passed / 0 failed, pnpm check:px-text clean. Rust: cargo test -p buzz-session-provider --lib 396 passed; cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib 2758 passed; cargo clippy -p buzz-session-provider --all-targets -D warnings clean; cargo clippy on desktop/src-tauri --all-targets -D warnings clean; cargo fmt --all --check clean; just file-size-check clean.
    - E2E — killed port 4173, `pnpm build:e2e`, `npx playwright test tests/e2e/crew-front-door.spec.ts --project=smoke` → 8 passed (18.8s). Test 03 renamed and extended: asserts the project sentence and the worktree toggle (data-state=checked) with name prefilled `team-roles-lead`. `shasum -a 256 test-results/swat1-poke/*.png` → all 16 hashes unique.
    - COMMITS (branch crew/front-door, 4 ahead of origin, NOT pushed, all Signed-off-by): 120a2e10 launcher project+worktree+navigate; da0d24d4 provider SEAT_CWD_SHARED for the app checkout; 2519a3f7 Stop all; 8574bd7d e2e test 03. Working tree clean.
    - NOT DONE (not in the brief): item 87(c), the Team tab's model/thinking picker — the lead still lands on whatever model the selected runtime/seat resolves, which is how it landed on opus[1m] rather than Fable.

    Gate lines from the landing run: gate1 `cargo test -p buzz-session-provider --lib` 396 passed / 0 failed; gate2 clippy clean; gate3 `cargo fmt --all --check` clean; gate4 desktop `pnpm test` 6675 passed / 0 failed; gate5 `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib` 2758 passed / 0 failed / 18 ignored; gate6 `pnpm check:px-text` clean; gate7 `just file-size-check` 9/9 checks passed; gate8 Playwright smoke 9 tests passed.

### Landed 2026-08-27 — "Bee Keeper" became "Beekeeper", three surfaces deliberately left behind

The display name is now one word everywhere (`d62bcb029` sweep,
`ccb06cf6c` installer cleanup, `73c40e165` Buzz leftovers; full `just ci`
green, bundle verified as `Beekeeper.app` with matching plist identity).
Machine identifiers were already one-word `beekeeper` and did not move. Two
consequences and three leftovers:

- **No migration for the managed-node dir** (decided): the literal in
  `desktop/src-tauri/src/managed_agents/managed_node_paths.rs` moved to
  `Application Support/Beekeeper`, so the first launch of a renamed build
  re-downloads the managed Node runtime and re-installs the ACP shims. The
  old `Application Support/Bee Keeper/` dir is orphaned, not deleted.
- `scripts/local-prod-build.sh` now quits and removes an installed
  `/Applications/Bee Keeper.app` before installing `Beekeeper.app` — the
  only intentional two-word strings left in the tree.
- **Still branded Buzz, deferred as bitmap/design work, not string edits:**
  `desktop/src-tauri/icons/dmg-background.png` (says "Buzz" in large baked-in
  type), the ASCII `buzz term` banner in
  `desktop/src/features/terminal/terminalBanner.ts` (needs new glyphs for
  k/p/n), and `README.md` (never got any rebrand pass). Package-name
  identifiers `desktop/package.json` `"buzz"` / `pubspec.yaml` `name: buzz`
  and the `desktop/public/buzz.svg` favicon filename were also left as-is.

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

**Read §2 item 84 first (2026-08-28 evening).** The designer seat is now
Banksy — the pack carries Brian's Banksy direction, a `see-the-app` drive
skill and a `wire-sources-for-surfaces` table that maps every UI fact to a
signed event kind or to honest unknown copy, and the installer asks a name
per identity ("Name your team") and republishes the kind:0 profile of any
identity it renames. Unit/DOM-level evidence only. **Next: Brian relaunches,
clicks Install team roles once** (renames plus the profile republish for
Banksy), **then hires Banksy as `designer`** with the Singularity mock and
design doc.

**Read §2 item 83 first (2026-08-28 evening).** The first hire from inside
Beekeeper seated a builder that could not report: the hire host granted it
nothing, and the relay refuses an ungranted seat's `sessions send`. Fixed on
`crew/front-door` (`cd482f7e`): the host publishes `grant-operator` after the
create receipt and discloses a failed grant to both the lead and the umbrella;
`bee sessions hire` now prints and returns `granted`, and the lead pack tells
the lead to read it. Unit-level evidence only — the next live hire is what
confirms the grant lands.

**Read §2 item 81 first (2026-08-28 evening).** D14 hire landed on
`crew/front-door`: a launch now seats the lead alone, and the lead hires the
rest with `bee sessions hire` (kind 44221 `session.hire`, authorized by the
relay). **This changes the relay**, so hive redeploys on the green pipeline;
until it does, the desktop and the CLI both print "this relay does not accept
hire requests yet" — that is the wire rule working, not a bug. After the
deploy: Brian relaunches the dev app, launches the lead alone from the Team
tab, and Keystone hires. Item 81 records the gate counts; **item 82 supersedes
its blocking residual** — the founder desktop's hire host is now mounted
(`AppShell.tsx:932`), model ids are checked against the runtime's catalog, and
a hire older than fifteen minutes is refused rather than seated. None of that
has been exercised live yet.

**Read §2 item 80 first (2026-08-28 afternoon).** Keystone's first mission
from inside Beekeeper found seven things by doing; three lanes on
`crew/front-door` fixed (a), (c), (d), (e) and (f), and item 80 names
exactly what is still open — (b) the pack union, (g)'s untested
interrupt/readdress, and the residuals each lane recorded.

**Andy, read this first (2026-08-28 morning):** the crew front door (item 76)
landed on `main` last night, together with the ledger entries for items 76 and
77. What that gives you: `Install crew roles…` on the team card, seating an
agent into a running session (join dialog, pending screen, session header),
`bee events query`, and `founder` / `createSigner` columns on `bee sessions
status` and `bee sessions list`. The installer also stages the designer and
poker packs as unseated roles, and item 76 records which surfaces this batch
deliberately did not build. Item 77 is the ledger of open findings from the
2026-08-27 live crew runs. **Also in this landing:** the fixes for items 73, 74 and 75 (pending-create honesty copy; every observer accepting a seated create; the provider briefing a seat with its pack) — they were cherry-picked into `crew/front-door` when the integration branch was cut, so they are on `main` as `02e17411`, `b76e22e0` and `b16dee88`; the topic branches `fix/pending-create-honesty` and `fix/seated-session-followups` are therefore redundant, not pending (`git log --oneline origin/main | grep -E "never confirmed|seated create|brief a seated"` shows all three). Three things that will bite you on a dev machine: (1) the
Beekeeper rename moved the managed runtime dir to `Application
Support/Beekeeper`, so the first launch re-provisions the managed Node
runtime and ACP shims — or copy the old
`Bee Keeper` dir across; (2) `~/.local/bin/bee` shadows the bundled `bee`
because `build_augmented_path` puts `~/.local/bin` first, so a seat can run an
old CLI unless you repoint that symlink; (3) seats still share the operator's
`~/.claude` (same `HOME`), so a seat's local cross-session tools can reach
other sessions — the fence item in 77 is not landed yet. Also open: the
"no-surface-by-decision" calls recorded in item 76 are waiting on Brian's
sign-off. **To mint a team on your machine (2026-08-28 evening):** Agents → the team card's menu → *Install team roles…* → *Choose folder…* and pick `<your checkout>/personas/roles` (the folder that contains `lead`, `architect`, `builder`, … — the parent, not one role) → name the lead → Install. The Agents tab now names a project itself — route, then your own pick from the *Role packs for: …* selector above the team cards, then the newest checkout this host has recorded, then your only/first project — so the folder is pre-chosen from that project's checkout dir (`<checkout>/personas/roles`) and the dialog label says which project it is ("The project's role packs — <name>"); *Choose folder…* still overrides it and retires the label, and a resolved project whose checkout is missing is still named rather than silently dropped — see items 85 and 86. Then New session → Team → Launch team seats the lead alone; the lead hires the rest with `bee sessions hire` (your desktop answers hires automatically; policy is on by default, 4 seats, installed roles). **Team launch, as of `8574bd7d` on `crew/front-door` (item 87):** opened from inside a project, the Team tab now says which project it is launching in ("Launching in <name>: …") and the create carries that `projectRef`, so the session shows up in the project's session list immediately; opened anywhere else it says so instead of guessing ("This session will not belong to a project…"). The tab also offers the same worktree toggle the one-session path does, on by default and prefilled `<session-slug>-lead`, so the lead no longer runs in the checkout the app runs from — and the provider refuses a seated cwd that is the app's own checkout (`SEAT_CWD_SHARED`), though that check only resolves when the desktop runs from inside a repo (`just dev`), not from a bundled `.app`. After a successful launch the lead's session opens instead of the dialog closing on nothing. The founder now gets **Stop all** in the session header ("Stop all (N)"), counting and stopping only live seats; the confirm says plainly that a stopped seat cannot be resumed. Still open: the Team tab has no model/thinking picker (item 87(c)/82), so the lead lands on whatever the selected runtime resolves.

**Read §2 item 78 first (2026-08-28).** The SWAT batch on `crew/front-door`
supersedes two of the three traps above: the installed team now launches
(vendor written on every seat, roster made launchable), and
`build_augmented_path` now ranks the bundled `bee` ahead of `~/.local/bin`.
The fence is briefing-only, not enforced. Nothing in that batch has been run
live — the next action is Brian's dogfooding walk, spelled out at the end of
item 78.

**Crew front door batch (§2 item 76) landed on `main` on 2026-08-28 after
Brian's go; relay code was untouched, so the hive deploy is a same-code
rebuild.**

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
cross-family pass §1 requires for a tier-2 diff. Proven live 2026-08-26 on the dev instance against hive (see §2 item 58
for the session and counts). Residuals: `DeliverError::Gone` and the no-live-actor arm still consume the
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
awaiting Brian's live look; not landed on main. Rebased onto
main@af3b9b66 and re-gated there on 2026-08-26 (`just ci` exit 0, desktop
6178/0, mobile 1465; `just test` exit 0, 2132 passed, 0 failed), pushed with
`--force-with-lease`. Rebased again onto main@b2298102 (the UX pass)
later that day with one docs conflict (this item is now 58) and re-gated:
**Slices 2–6 were built by a Claude-only crew on crew/s2-s6; as of 2026-08-27
all five are reached, S2–S4 and S6 are gated green, and S5 is green but
carries two advisory notes.** S2 (the relay is the mailbox) is gated and
proven live at `1a3ee24d`. S3 (agent seats) was gated green by the lead on
`298a69f6`. S4 (agents talk) is green at `cbbdf7e2` and is on the relay. S5
(role packs and crew launch) was re-gated green at `d7eca684` after round 3
closed both blocking findings — the `"default"` adapter alias now resolves to
vendor `unknown`, and `materialize_skills` resolves its destination the way it
resolves its source. S6 (budgets and liveness) is built through `83c386fc` and
**gated green**: `cargo test -p <crate> --lib` over 8 crates 3708 passed / 0
failed (the run `just ci` does not make), clippy clean, desktop 6365/0 over 72
suites, Tauri 2712/0/18 plus 7 and 3, px-text clean, `just test` 12/12. Its
same-family refuter returned CONFIRMED; two fix-now findings were applied
(contract-1, an agent seat could mint the founder exemption the budget bounds;
contract-4, an out-of-range `BUZZ_CSP_TURN_BUDGET` silently dropped every
budgeted 44223 in the desktop) and five are deferred (contract-1b, -2, -3, -5,
-6). **Nothing here has run against a relay or a real provider**, no full
`just ci` has run on this branch, every refuter was same-family (advisory, not
the cross-family tier-2 pass §1 requires), and the checkpoint could not push:
the pre-push branch-skew guard reports the branch behind `origin/main` on 24
files it also touches, and hermit `just` was missing from the hook subshell so
six other hook steps exited 127 before it. What is owed before any of this
lands: a reconciliation with the moved `origin/main`, a full repository gate,
and one crew launch watched in the app. Rebased onto main@fecf0d33 (Andy's
stall/redaction work) 2026-08-27 and re-gated there: lib 8 crates green
(buzz-relay alone 959/0 excluding the pre-existing demo_join failure fixed on
crew/relay-cancel-safety; the 12 failures seen under a parallel nine-step gate
were Sqlx PoolTimedOut contention and vanished alone), clippy clean, desktop
6401/0, Tauri 2718/0, just check, mobile 1465, px, just test 12/12; pushed to
`origin/crew/s2-s6` — tip **run integration tests alone, not inside a
parallel gate, when a count looks off; Sqlx PoolTimedOut contention under
concurrency reads as real failures but vanishes solo.**

**The active track as of 2026-08-25 night is live confirmation of the
full-screen UI/UX pass — §2 items 52–53 and 55–57.** The implementation,
focused wide-screen E2E workflow, and repository-wide `just ci` gate are green.
It landed on `main` as `b2298102` on 2026-08-26 (rebased onto Andy's
`af3b9b66`, gated there, fast-forwarded); only confirmation in the live
desktop against hive remains. Everything below is the previous track, kept
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
  pipelines too and can deploy **stock upstream Buzz onto a Beekeeper relay**,
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
  `a0860552c` — a Beekeeper commit, mid-build. Had it gone green first, an
  unpinned deployer would have put Beekeeper on the vanilla relay, and it
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
- **The relay ships before the desktop for coding-session turns (crew S2).**
  A `kind:44220` `thread.turn.start` now carries an optional `deliver` key, the
  desktop builder always writes it
  (`desktop/src/features/coding-sessions/lib/codingSessionCommand.ts`), and the
  relay validates 44220 content with `deny_unknown_fields`
  (`crates/buzz-relay/src/handlers/ingest.rs`,
  `crates/buzz-core/src/coding_session_command.rs`). An upgraded desktop
  against an older relay therefore has **every** turn rejected, and NIP-CSC
  forbids the kind-9 fallback that would have hidden it. Deploy the relay
  first, then the desktop; the reverse order is a total turn-sending outage for
  that community. See `docs/nips/NIP-CSC.md` § Deploying the `deliver` key.
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
