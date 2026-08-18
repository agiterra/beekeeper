# Handoff: provider-neutral sessions → Sol

**Written** 2026-08-13 by the Claude session that did the work described below.
**For** Sol (Codex / gpt-5.6-sol) taking over the effort.
**Status of the work** Steps 1 and 4 of the roadmap are implemented, gated, and
proven live. Nothing is pushed. The next moves are listed in §8 and §9.

---

## 1. What this effort is

Buzz's coding-session surface began as a Claude-only feature. The goal is the
product described in `docs/SESSION_VISION.md` (Andy's brief, read it first and
in full):

> A session is a durable, shared environment where humans, agents, tools, and
> artifacts work together around the work at hand.

Providers (Claude Code, Codex, Goose, …) *participate* in a session; they do not
own or define it. The session belongs to its participants and its history.

Brian's own framing, which corrected an early misreading and should be treated
as binding: **the thesis is both providers inside ONE session as
co-participants — not parallel sessions you flip between.**

Ten product invariants are listed at the end of the vision doc. The ones that
have bitten us in practice: **#2** (one provider must stay effortless — no
multi-agent ceremony in the single-agent case), **#5** (everything
attributable), **#8** (capabilities honest — never show a control a provider
does not have, never claim enforcement that does not exist), and **#9**
(sensitive execution state stays protected — see §7, we found real violations).

---

## 2. Where everything lives

### Repos and worktrees

| Path | Branch | Role |
|---|---|---|
| `~/agiterra/BuzzForkV2` | `integrated` | Generated assembly. Runs the daily product. **Has uncommitted local work — never commit here.** |
| `~/agiterra/BuzzForkV2-coding-sessions` | `feature/coding-sessions` | Where all session code lands. Upstreamable, no projects glue. |
| `~/agiterra/BuzzForkV2-integration-glue` | `integration/glue` | Cross-feature adaptation + all fork-only docs, including this one. |
| `~/agiterra/t3code` | — | T3 Code source, **MIT**, reference material (§6). |

### Documentation situation (honest assessment)

Fork docs live in `docs/` on `integration/glue`. Upstream subdirs carry the
formal contracts: **`docs/nips/`** is the important one — every wire contract is
specified there (`NIP-CSL.md` lifecycle, `NIP-CST.md` transcript, `NIP-CSPC.md`
provider catalog, `NIP-CSC.md` commands). Amend the NIP whenever you change a
payload; we did this for `sessionRef`.

The session documents added by this effort:

| Doc | What it is |
|---|---|
| `SESSION_VISION.md` | **Durable.** Andy's product brief + invariants. Do not rewrite. |
| `SESSION_PATH.md` | Roadmap Steps 0–5, plus appendices that accreted: T3 borrow rules, managed-agent interop doctrine, execution-sandboxing findings. |
| `SESSION_STEP4_DESIGN.md` | Umbrella-session design + post-implementation review notes + accepted risks. |
| `SESSION_HANDOFF_SOL.md` | This file. |

**Known weakness, worth fixing early:** we have no separation between *durable
reference* and *in-flight working plans*, so `SESSION_PATH.md` has grown
appendices that are really work items. T3 solves this with `.plans/` (32
numbered working docs with a status legend: `required` / `implemented` /
`to-replace` / `delete`) separate from `docs/` (architecture / internals /
operations / user). A proposed reorg — `docs/plans/NN-*.md` with a README index,
fork-only, promoting stabilized contracts into `docs/nips/` — was drafted but
**not executed**; Brian has not approved it. Ask before moving files.

---

## 3. Git ceremony (Andy's workflow — follow exactly)

| Branch | Rule |
|---|---|
| `main` | Pure fast-forward mirror of upstream `block/buzz`. **Never** put local work here. |
| `feature/<name>` | One upstreamable feature, based on `main`. Session code goes on `feature/coding-sessions`. |
| `integration/glue` | Cross-feature adaptation, fork-only docs and tooling. |
| `integrated` | Generated assembly. Rebuilt and force-pushed. Never commit to it. |
| `build/YYYY-MM-DD[.n]` | Immutable pins of known-good assemblies. |

Working rules that are not negotiable:

- **Activate hermit before any git or cargo command:** `. ./bin/activate-hermit`.
- **Sign every commit:** `git commit -s` (DCO check fails the PR otherwise).
- **Do not push.** Brian batches work to avoid triggering repeated integration
  builds. Push only when Brian or Andy explicitly asks for the ceremony.
- **The ceremony** = merge the feature stack, rebase the glue patch series over
  the assembly, run the gate, tag `build/*`, force-push the assembled branches.
- Consumers of `integrated` re-fetch or pin a `build/*` tag.
- Known hook gotcha: pre-commit runs `just desktop-tauri-fmt`, which can fail in
  a worktree for path reasons. Run `cargo fmt --manifest-path
  desktop/src-tauri/Cargo.toml` yourself; only then, and only for that specific
  failure, use `--no-verify` and say so in the commit body.

**Relay deployment is downstream of the ceremony.** `lightyear.agiterra.org` is
the only live relay and it runs a binary built from this repo. `.github/workflows/docker.yml`
builds a relay image on `main` pushes and `relay-v*` tags — but `main` is an
upstream mirror, so an image from `main` contains none of our work. Our relay
changes reach lightyear only by building from the assembly. Agreed model:
**local `:3000` is the test relay; the ceremony updates lightyear for Brian and
Andy together.**

---

## 4. Environment runbook (hard-won; saves hours)

- **Run the app:** `cd ~/agiterra/BuzzForkV2-coding-sessions && . ./bin/activate-hermit && just desktop-standalone`.
  **Not** `just dev` — that demands Docker/colima services and its own relay on
  :3000, while this machine runs native Homebrew Postgres/Redis. Launch from a
  terminal so the app inherits a PATH containing the adapters.
- The standalone instance is isolated: own keyring, own app-data dir
  (`~/Library/Application Support/xyz.block.buzz.app.dev.feature-coding-sessions`),
  worktree-derived Vite port (29058). First launch asks for credentials and a
  community — that is expected, not a bug.
- **Local test relay:** build from this worktree (`cargo build -p buzz-relay`),
  run `BUZZ_GIT_CONFORMANCE_PROBE=false ./target/debug/buzz-relay`. The probe
  gate must be disabled when MinIO/Docker is down; consequence is degraded
  git/media, which coding sessions do not need.
- **The sidecar must be rebuilt after Rust changes:** `cargo build -p
  buzz-session-provider`; the desktop resolves it from `target/debug`.
- **Provider logs:** `<app-data>/session-provider/logs/<pubkey>.log` — this is
  the first place to look for anything session-related.
- **Codex is installed and logged in** via Brian's ChatGPT subscription
  (`codex` 0.147.0, model `gpt-5.6-sol`, reasoning effort high, from
  `~/.codex/config.toml`). The ACP adapter `@agentclientprotocol/codex-acp`
  1.2.0 is installed globally.
- **On `feature/coding-sessions` there is no projects-container UI** — sessions
  are reached from the **channel header** menu. Upstream's repo-oriented
  `projects` feature exists but needs MinIO.
- **Economic model:** everything runs on subscriptions. An execution runs on the
  machine and provider login of whoever launched it. A teammate "continuing" a
  session means a new execution on *their* subscription. No shared keys, no
  metering to build.

---

## 5. What was built (all on `feature/coding-sessions`, unpushed)

| Commit | What |
|---|---|
| `1b90f5c4` | Generalize the session provider to N ACP runtimes (shared `RuntimeDescriptor`, `BUZZ_CSP_RUNTIMES`, catalog emits one entry per runtime, create routes `providerInstanceRef` → adapter). Zero-config default stays claude-only, byte-identical. |
| `d32b104d` | Runtime picker in the create flow; structured turn-result cost/duration consumed as fields. |
| `98230044` | Re-arm trusted ingress when the provider trust list changes (first-run race: provisioning seeds the trust entry *after* the subscription armed, so the create screen spun forever). |
| `f6a285cb` | `sessionRef` (umbrella id) through create → session record → metadata. Additive: decoder accepts the legacy 8-key and new 9-key action. NIP-CSL amended. |
| `c66ce2cf` | Umbrella model, interleaved timeline, `cs-session` conversation lane, handoff helpers, create-observation collection (founder/operator authority joined via receipt `commandId`). |
| `d09f0d95` | "Add provider" dialog + umbrella surface + participant selector + handoff chips. |
| `ff29584b` | Lane visibility rule applied across four timeline paths, unread, notifications, cross-community observer, Home mentions; `useUnreadChannels` split for the file-size guard. |
| `3825ff77` | `@mention` sugar — `@claude` / `@codex` at the start of a message retargets and strips the handle. |
| `d446e29e` | Session ingress converges through relay back-pressure (see below). |
| `61855290` | **Env fence**: stop leaking provider credentials into agent adapters (§7). |

Also on `integration/glue`: `c212d427`, `0bf1368a`, `2f0faabd`, `e876bd27` (the
docs above).

**Proven live on 2026-08-13**: one session holding a Claude execution *and* a
Codex execution, participant selector, `@mention` routing, handoff chip, and
Codex's native reasoning summaries / skill reads / web search rendering through
the provider-neutral transcript contract unchanged.

### Bugs found by running it (each one instructive)

1. **Old relay rejects new creates.** The relay does not merely store the two
   operator command kinds — it *decodes and validates the action payload*
   fail-closed (`crates/buzz-relay/src/handlers/ingest.rs:1871-1876`). A relay
   built before `sessionRef` existed rejects every create with
   `"invalid: coding-session lifecycle command action has missing or unsupported
   fields"`. This is why local `:3000` had to be rebuilt. A partial client-side
   "legacy create shape" fallback is **git-stashed** on the feature branch
   (`wip: relay-compat legacy create fallback`) — incomplete, optional under the
   ceremony model.
2. **Rate limiting misclassified as fatal.** Entering a session view fires a
   burst of REQ frames (trusted ingress, create observations, lane, plus channel
   window and backfills) against a ~50-frames-per-5s budget. The client
   recognized only one of the relay's three `rate-limited:` phrasings as
   retryable; the other two latched as fatal, so the transcript rendered
   "Generation not found" for a live session. Fixed in `d446e29e`, but **the
   burst itself is unaddressed** — the umbrella work roughly doubled the frames
   per session view. Collapsing each hook's `fetchEvents` + `subscribeLive` pair
   into one REQ would halve it, at the cost of the EOSE-based loading signal.

---

## 6. T3 Code findings (reference implementation)

`~/agiterra/t3code` is **MIT** (compatible with this repo's Apache-2.0 and with
DCO clause b). Borrow freely; retain the MIT notice for substantial ports
(`Portions derived from t3code, © 2026 T3 Tools Inc., MIT License`). Borrow
from the source repo, not the shipped app bundle (which carries no grant).

Their backend is Effect-TS over local SQLite projections; ours is Rust over a
signed multi-client relay. **Borrow their logical flow, not their architecture.**

### Where they are ahead (adopt the flow)

- **Session survives provider death.** A thread is provider-independent; the
  runtime binding is a separate row with a `resume_cursor_json`, and the next
  turn lazily reattaches (Codex literally re-opens the same provider thread).
  Ours retires every session on restart — and since the Tauri supervisor
  auto-restarts the sidecar, **every supervised restart kills every live
  session**. We have `generation` on the wire and fence commands against it, but
  it is hardcoded to 1 and never incremented: the vocabulary exists, unused.
- **Permission modes.** One neutral `RuntimeMode` — `approval-required`
  (labelled "Supervised") / `auto-accept-edits` / `auto` / `full-access` —
  stored on thread *and* session, changeable mid-session (implemented as
  restart-with-resume), mapped per provider: Codex → `approvalPolicy` +
  `sandbox` + `approvalsReviewer` (and a per-*turn* sandbox policy); Claude →
  `PermissionMode` (`acceptEdits`/`auto`/`bypassPermissions`, with
  `approval-required` deliberately unmapped so the SDK default asks). Their UI
  copy is honest about heterogeneity: *"Supported providers approve routine
  actions; others still ask."*
- **A full approval / user-input round trip**: native hook → deferred → activity
  → pending-approval projection → shell counter → composer UI → decision command
  → deferred resolves → native callback returns.
- **Attention/triage for many sessions**: `pending_approval_count`,
  `pending_user_input_count`, `has_actionable_proposed_plan`, plus
  pinned/snoozed/settled/archived with a ranked status precedence and blockers
  that prevent auto-settling a thread that is waiting on a human.
- **Real workspace diffs**: git checkpoints written to hidden refs
  (`refs/t3/checkpoints/<thread>/turn/<n>`) using a temp index — never touches
  HEAD, index, or stash — with live diff computation and a revert that restores
  the filesystem *and* rolls back the provider conversation.
- **Turn folding** (settled turns collapse to "Worked for 2m 14s"), and
  **server-side payload slimming** (~84-char tool summaries to the client, full
  payload kept in storage).
- **Provider lifecycle**: version checks, update execution, status caching,
  account identity from the auth probe, multiple instances per driver.
- **Forward-compatible schemas** (optional fields, decoding defaults) — the lack
  of which is exactly why our 9-key create was rejected by an older relay.

### Where we are ahead (do not regress)

Signed multi-client truth with per-generation authority; umbrella sessions
across executions (T3 has no cross-thread grouping at all); generation fencing;
capabilities advertised identically in catalog *and* metadata so picker and
header cannot disagree; canonical catalog bytes with honest revision semantics;
`elided`/quarantine transparency; explicit `isError`; transcript virtualization;
per-turn provider-attested cost; the adapter env fence (no T3 analogue);
reasoning/thought rendering.

### Do not copy

`checkpoint_diff_blobs` (a table created by migration and never read or written
— vestigial spec drift); server-owned worktree restore (assumes a single writer
and one machine owning the workdir); dropping superseded activity rows (we have
a signed archive — *project* a slim view, never delete). And note their
attention state is stored per-thread because they have exactly one user; **ours
must be per-(viewer, session)**.

### Two corrections to earlier claims in this repo's notes

- Kinds **46010–46012 are workflow approval kinds**, not coding-session ones.
  Nothing in 44220–44225 is reserved for approvals; new kinds are required.
- `SessionStatus::WaitingForInput` is **declared and fully wired in the
  frontend** but has **zero backend producers** — a dead enum variant waiting
  for a signal.

---

## 7. Security findings (from an in-session agent auditing itself)

An agent inside a Buzz coding session was asked to audit its own environment.
Confirmed in code:

1. **The adapter inherited the provider's signing key.** `BUZZ_PRIVATE_KEY` (the
   pubkey the desktop fail-closed-trusts for kinds 44222–44225) plus
   `BUZZ_AUTH_TAG` (the NIP-OA owner delegation) were visible to the agent — so
   an agent could sign transcript facts the UI renders as authentic, carrying
   the owner's standing. **Fixed** in `61855290` via an opt-in `EnvFence` that
   scrubs the whole `BUZZ_*` prefix plus enumerated secrets at the
   sidecar→adapter boundary, applied to both the session spawn and model
   discovery. The managed-agent harness path is deliberately unchanged (a
   managed agent *is* a Buzz participant and should act as itself).
   Intended consequence: the `buzz` CLI no longer authenticates from inside a
   coding-session shell. That is correct — an execution should get its **own**
   identity via `agent_ref`, not borrow the provider's.
2. **App infrastructure secrets leaked from the developer's `.env`**
   (`BUZZ_S3_*`, `TYPESENSE_API_KEY`) — same fix.
3. **The agent inherits the operator's personal MCP servers** — in that audit,
   authenticated Gmail and Google Calendar, plus browser control with arbitrary
   script evaluation. **Not fixed. Env scrubbing cannot fix it**: it comes from
   the provider CLI reading the operator's user-level config. Requires launching
   the adapter against a scoped config/profile directory. **This blocks Step 3/4
   sharing** — a steerable shared session must not be a path to the operator's
   personal integrations.
4. **Permission decisions are hardcoded auto-approve** in the *transport*:
   `crates/buzz-acp/src/acp.rs` answers every `session/request_permission` with
   `allow_once`, synchronously, inside the blocking read loop, with no channel
   out. A permission UI cannot be attached without restructuring that loop.
   This is the root blocker for the whole approval story.
5. Filesystem scope is the user's, not the session's. Inherent to shell-capable
   agents; noted so nobody mistakes the working directory for a boundary.

---

## 8. Ranked queue (my recommendation, post-T3-study)

1. **Resume + generations.** Increment `generation` on reattach, persist the ACP
   session id we already have as a resume cursor, add `session.resume` and
   `session.stop` lifecycle actions. Fixes the worst structural weakness (every
   restart kills every session) and gets worse once sessions are shared.
2. **Permission surface.** Move the decision locus out of the blocking ACP read
   loop into an async channel; add a neutral runtime mode to the descriptor and
   the create payload; add approval + user-input transcript item kinds and the
   response command; produce `WaitingForInput`. Largest piece; everything
   downstream depends on it.
3. **Schema evolution policy.** Move payload decoding from exact-key matching to
   optional-with-defaults, so additive fields stop being breaking changes.
4. **Transcript truth.** Provider-emitted turn diffs (replacing client-side
   inference from tool arguments, which misses shell edits, deletions, renames);
   a slimming projection between the signed archive and the render model;
   turn folding; a context-window meter; show the model that actually took
   effect instead of the `"default"` alias.
5. **Attention.** Per-viewer counters and lifecycle state, once approvals exist
   to count.
6. **Smaller, known:** MCP profile isolation (§7.3, blocks sharing); collapse
   the doubled subscription burst; handoff chip overlaps the participant
   selector (layout); virtualize the umbrella timeline.

Roadmap steps still open in `SESSION_PATH.md`: Step 2 remainder (modeled diffs),
Step 3 (observer mode), Step 4.5 (@mention sugar landed; **persistent session
agents via `agent_ref` → kind 30177 remains**), Step 5 (checkpoints, continuity).

---

## 9. Sol's mission

Beyond the queue: **find where all of this already exists in Buzz.** This
codebase is much larger than the session feature, and the effort so far has
repeatedly discovered that Buzz already had a primitive we were about to invent
(the provider catalog already allowed 32 providers; `KNOWN_ACP_RUNTIMES` already
described four runtimes; agent-to-agent messaging already worked via verified
same-owner siblings; the conversation lane turned out to be ordinary `kind:9`
chat, which is exactly the protocol managed agents already speak).

Specific hunts worth running before building anything from §8:

- **Approvals/permissions**: `buzz-workflow` has `request_approval` steps,
  `StepResult::Suspended`, approval tokens, and kinds 46010–46012 — all stubbed
  (`WF-08`). Is that machinery the right home for session approvals, or a
  parallel design to learn from? What does `feed.rs`'s needs-action query do?
- **Attention/triage**: Buzz already has unread tracking, mention counting,
  notifications, and a Home/inbox feed. Session attention should almost
  certainly ride those rails rather than invent a second system.
- **Persistent identity**: `KIND_MANAGED_AGENT` (30177), engrams (30174),
  personas (30175), teams, NIP-OA delegation, and `is_owner_or_sibling` in
  `buzz-acp`. This is the substrate for `agent_ref` and for the merge-captain
  pattern in `SESSION_PATH.md`.
- **Checkpoints/diffs**: the relay *hosts git* (smart HTTP, object storage,
  signed objects via `git-sign-nostr`). A session branch or snapshot ref may be
  a better continuity mechanism than anything client-side.
- **Continuity/resume**: does ACP expose `session/load`? What does
  `buzz-acp`'s pool/queue do about reconnection that the session provider does
  not reuse?
- **Rate limits**: `buzz-auth`'s `rate_limit.rs` and the relay's `admission.rs`
  govern the burst problem; there may be a subscription pattern already used
  elsewhere in the app that avoids it.

Write findings down as documents (see §2 on the docs situation, and ask before
reorganizing). Prefer modelling new operations as Nostr event kinds over new
HTTP endpoints — that is the repo's stated architectural rule, and it buys
realtime fan-out, NIP-29 scoping, and the existing auth pipeline for free.

---

## 10. Working agreements with Brian

- He is a manager, not a hands-on coder. He sets direction; you implement.
  **Make the call** rather than presenting options. Report outcomes: *"Did X.
  Result. Next: Y."* Terse is good.
- Check in only for genuine irreversibility: destroying others' work, external
  communications that notify a person, money/deploys, direction changes,
  production. Everything reversible by another commit — just do it.
- He notices and cares about honesty in the product: a control that lies about
  what it enforces, a badge for a message you cannot find, a "default" label
  that hides the real model. Several of the best findings in this effort came
  from him poking at exactly those.
- Multi-agent orchestration is pre-authorized for this effort and he likes
  watching it run. The pattern that worked: written spec → parallel build lanes
  with **strict file ownership** → full gate → adversarial review against named
  constraints → a single finalizer that commits. Lanes never commit; only the
  finalizer does.
- Before messaging Andy or anyone else, draft it and get Brian's go.
