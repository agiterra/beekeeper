---
name: beekeeper-project
description: "Project overlay: how a lead works on Beekeeper itself — git, the ledger, the wire, the team model, and what the operator will not accept."
---

# Beekeeper — the project overlay

Base `lead` says how to lead. This says how this project works, and it **carries
the project's rules itself** — you do not read `docs/SESSION_STATE.md` whole to
find them. Anything below that also lives in `AGENTS.md` (loaded into your
context already) is a pointer, not a restatement; read the named section there
rather than a second copy here.

## The ledger — read §3, and only the items your brief cites

`docs/SESSION_STATE.md` is the living record: §1 what is live, §2 the open
findings (numbered, each with the code or transcript that proves it), §3 `Next`
(the ordered track), §3a environment facts that already cost hours.

**Read §3 Next, and only the numbered §2 items your brief cites. Never the whole
file.** It grows every ceremony — 5,500 lines and counting: a seat that reads it
start to finish spends about a quarter of its context window before it has done
anything, and a codex seat already spends ~25% at boot (ledger item 80f). §3 is not at the top — jump to it:

```
grep -n '^## ' docs/SESSION_STATE.md                       # section line numbers
sed -n '/^## 3\. Next/,/^## 3a\./p' docs/SESSION_STATE.md  # the track, ~240 lines
grep -n '^79\. ' docs/SESSION_STATE.md                     # where item 79 starts
sed -n '<start>,<start+100>p' docs/SESSION_STATE.md        # read that window only
```

(Do not range an item to the next number — the last item has no successor and
the range runs to EOF, which is the whole file again.)

Where this skill and the ledger disagree about *current state*, the ledger wins
— but ask it about one thing, not everything. Findings go **into** the ledger,
never into a new handoff document. A team's in-flight dispositions go on the
wire as Pulse entries (kind 44240); the repo document is for what landed.

Brief your own lanes the same way: cite ledger items by number so a lane reads
those items and nothing else.

## Git is relay-canonical

`AGENTS.md` (the block at the top) has the remotes, the rebase-not-merge rule,
`git commit -s`, and `just install-git-credentials`. Two things it does not say:

- **Push to `origin` only.** The forge's mirror bridge pushes every ref to
  GitHub within seconds and GitHub triggers CI; pushing to GitHub directly
  races the bridge.
- **A push that returns HTTP 401 after green pre-push hooks is ledger item 71**,
  not a broken credential: git mints the NIP-98 credential during ref
  discovery, so a long hook window expires it. Retry **once** with
  `--no-verify` on the identical SHA — never on a SHA the hooks did not see.

And the one the run of 2026-09-02 made necessary:

```
You never push `main`. A branch is landed by the founder, or by a seat the founder names in the policy's `irreversible` list, after a verifier's report and your verdict are on the wire.
```

This is a pack rule because nothing else stops you. A seat inherits the
operator's repo role — Owner, here — so the relay's git gate will accept your
push to `main`; there is no `buzz-protect` rule on that ref, and the push path
reads no verdict. On 2026-09-02 at 12:00:19 a lead landed `main` after its own
verifier had reported FAIL, and every layer below it said yes. Until the gate
exists (ledger item 108, lane L6), the rule lives here and the guard in
`crates/buzz-persona/tests/pack_rules.rs` keeps it here.

## Worktrees, and the operator's live checkout

Every lane works in its own worktree under
`/Users/brian/Projects/beekeeper/beekeeper.worktrees/<lane>`. The main checkout
`/Users/brian/Projects/beekeeper/beekeeper` is **hot**: Brian's dev app runs
from it and `tauri dev` rebuilds on write. Never edit, rebase or switch
branches there — a seat that does is editing under a running app. A seat handed
that cwd stops and says so (ledger item 80a).

Before any `cargo`/`pnpm`/`just`/git-hook command:
`cd <worktree> && . ./bin/activate-hermit`. Do not rewrite hook commands to
work around an unconfigured `PATH`.

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
- Event kinds over new HTTP endpoints, and channel scoping by `h` tag: see
  `AGENTS.md` § Key Patterns.

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

`AGENTS.md` § Quality Gates is the contract — `just ci` before any PR (fmt and
clippy are separate), `just test` for `buzz-relay`/`buzz-db`/`buzz-auth`, no
`unsafe`, no new `unwrap()`/`expect()` in production paths, doc comments on new
public API. Desktop text uses rem tokens only (§ Text sizing & zoom); every
file stays under 1000 lines (`just file-size-check`) — split it, never raise
the limit. Nothing in this file relaxes any of it.

## What the operator will not accept

`AGENTS.md` § Working agreements is the source: make the call instead of
offering a menu, cite `file:line` or the run, prefer an unpleasant truth over a
comfortable guess, and check in only for genuine irreversibility. Three things
that section does not say:

- **The word "crew" in anything a person reads.** It is "team" now. Internal
  identifiers follow when you are already editing the line.
- **Approving on a report alone.** Run the lane's acceptance yourself and read
  the value it produced — see `skills/triage-report`.
- **Absence is not a claim.** Say nothing rather than guess.
