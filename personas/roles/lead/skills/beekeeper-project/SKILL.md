---
name: beekeeper-project
description: "Project overlay: how a lead works on Beekeeper itself — git, the ledger, the wire, the team model, and what the operator will not accept."
---

# Beekeeper — the project overlay

Base `lead` says how to lead. This says how this project works. Where the two
disagree about *current state*, `docs/SESSION_STATE.md` wins — read it first,
every session.

## Git is relay-canonical

- Three remotes: `origin` = the relay's own git hosting (`hive.agiterra.org`),
  `upstream` = the GitHub mirror CI watches, `vanilla` = the block/buzz mirror.
  The names moved on 2026-08-24; run `git remote -v` rather than trusting a
  document. **Never hard-code a remote name in tooling** — two pre-push guards
  did and both broke silently.
- **Push to `origin` only.** The forge's mirror bridge pushes every ref to
  GitHub within seconds and GitHub triggers CI. Pushing to GitHub directly
  races the bridge.
- Pushing needs Nostr credentials: `just install-git-credentials` (NIP-98, not
  a password). Without the helper `git fetch origin` waits on a username prompt
  that can never be answered. `bee git status` says whether it is set up.
- Topic branches are **rebased** onto `main`, never merged into it:
  `git rebase --signoff main`, then force-push the topic branch. `vanilla/main`
  is the exception — it is merged, never rebased.
- Every commit is signed off (`git commit -s`); the DCO gate fails a PR
  without the trailer, and rebase/cherry-pick need `--signoff` explicitly.
- **A push that returns HTTP 401 after green pre-push hooks is ledger item 71**,
  not a broken credential: git mints the NIP-98 credential during ref
  discovery, so a long hook window expires it. Retry **once** with
  `--no-verify` on the identical SHA — never on a SHA the hooks did not see.

## Worktrees, and the operator's live checkout

Every lane works in its own worktree under
`/Users/brian/Projects/beekeeper/beekeeper.worktrees/<lane>`. The main checkout
`/Users/brian/Projects/beekeeper/beekeeper` is **hot**: Brian's dev app runs
from it and `tauri dev` rebuilds on write. Never edit, rebase or switch
branches there — a seat that does is editing under a running app.

Before any `cargo`/`pnpm`/`just`/git-hook command:
`cd <worktree> && . ./bin/activate-hermit`. Do not rewrite hook commands to
work around an unconfigured `PATH`.

## The ledger

`docs/SESSION_STATE.md` is the living record: what is deployed, what is open
(each item with the code or transcript that proves it), and the environment
facts that already cost hours. Findings go **into** it, never into a new
handoff document. A team's in-flight dispositions go on the wire as Pulse
entries (kind 44240) — the repo document is for what landed.

## The wire

- Coding sessions are signed Nostr events, kinds **44220–44230**: 44220 turn
  command, 44221 lifecycle command (create/stop), 44222 provider catalog,
  44223 session metadata (status, seat `actor`/`role`, model, runtime),
  44224 lifecycle receipt, 44225 transcript, 44226 genesis, 44227 goal,
  44229 name, 44230 closure.
- **Receipts are per stage** and keyed by `commandId`: `turn_queued`,
  `turn_started`, `turn_dropped`, `turn_refused`. A stage nobody published did
  not happen; do not infer it from a transcript.
- **A role slug is unique only inside one umbrella.** `bee sessions send
  --to <role>` is refused without `--session-ref`.
- Prefer a new event kind (`buzz-core/src/kind.rs`) over a new HTTP endpoint.
- Channel-scoped events carry `h` tags; addressable events describing a
  channel carry the id in `d`.

## The team model (plan §3.1, D11–D16)

- **D11** Identities are durable and named; seats are ephemeral. Hire from a
  roster of named identities per role so a diff is signed by someone with
  continuity. An agent never holds keys — you *request* a hire; the host mints
  and seats under the operator's pre-authorised policy.
- **D12** Role is fixed per identity; seat role = home role. Packs are
  versioned by project ref (`<repo>@<commit>:personas/roles/<role>`), with a
  project overlay — this file is the lead's.
- **D13** Model and thinking are per seat, by the `choose-model` rubric, and
  you say why in the hire.
- **D14** You hire the team after hearing the mission: goal → proposed roster
  (identities, models, why) → operator or policy approves → the host seats.
- **D15** A "playbook" (High-velocity SWAT, Surgical) is a pack overlay, not a
  team.
- **D16** The Agents screen is the hub, **desktop first**; mobile and web are
  notated and revisited.

## Quality gates

`just ci` before any PR (fmt and clippy are separate — passing one is not
passing the other). `just test` if you touched `buzz-relay`, `buzz-db` or
`buzz-auth` (needs Postgres + Redis). No `unsafe`; no new `unwrap()`/`expect()`
in production paths; doc comments on new public API; desktop text uses rem
tokens only (`pnpm check:px-text`); every file stays under 1000 lines
(`just file-size-check`) — split it, never raise the limit.

## What the operator will not accept

- **The word "crew" in anything a person reads.** It is "team" now. Internal
  identifiers follow when you are already editing the line.
- **A menu of options.** Decide, act, report: *"Did X. Result. Next: Y."*
- **A completion report without evidence.** `file:line`, a SHA, or an exit
  code — or it did not happen. This applies to your own prior claims and to
  every document in `docs/`.
- **A comfortable guess in the product.** A control that lies about what it
  enforces, a badge pointing at a message you cannot find, a status reading
  Idle over a disconnected provider — those are bugs of crash severity.
  Absence is not a claim: say nothing rather than guess.
- **Approving on a report alone.** Run the acceptance, read the value.

Check in only for genuine irreversibility: destroying someone else's work,
outward-facing communication, money or production deploys, or a change of
direction. Anything another commit can undo — just do it.
