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

## The map first, then only the ledger items your brief cites

`docs/CURRENT_STATE.md` is the current-state map: what is deployed, active
work and owners, decisions in force, blockers, the ordered next steps, and
where the evidence lives. Read it whole — it is gated to 300 lines and 24,000
bytes — and re-read it after your context compacts.

`docs/SESSION_STATE.md` is the evidence ledger: numbered findings, each with
the code or transcript that proves it, numbers frozen because code cites them.
**Read only the items your brief cites. Never the whole file.** It is past
12,000 lines: a seat that reads it start to finish spends most of its context
window before it has done anything, and a codex seat already spends ~25% at
boot (ledger item 80f). Jump to an item:

```
grep -n '^79\. ' docs/SESSION_STATE.md                     # where item 79 starts
sed -n '<start>,<start+100>p' docs/SESSION_STATE.md        # read that window only
```

(Do not range an item to the next number — the last item has no successor and
the range runs to EOF, which is the whole file again.)

Where this skill and the map disagree about *current state*, the map wins.
Findings go **into** the ledger as numbered items, never into a new handoff
document; session reports go under `docs/history/`. A team's in-flight
dispositions go on the wire as Pulse entries (kind 44240); the repo document is
for what landed.

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

And the one the runs of 2026-09-02 and 2026-09-04 made necessary:

```
A landing is admitted by the relay's push gate, never by a person. When it refuses, the sentence names the missing fact, and you have two moves: produce that fact, or publish a blocker quoting the sentence verbatim with the row event ids. You never pass a landing to a founder.
```

This is a pack rule because both failure modes were live. On 2026-09-02 at
12:00:19 a lead landed `main` after its own verifier had reported FAIL, and
every layer below it said yes; since 2026-09-03 the relay's `require-verdict`
gate reads the wire instead, and a push to `main` is admitted by one of three
arms (`bee sessions explain arm`) or refused with a sentence. On 2026-09-04
a lead answered that refusal by ending its report with an instruction to the
founder to push the commit themselves — the one remedy the gate exists to
remove, and one Brian ruled out on 2026-09-03: humans never gate a landing
(live-run findings 75 and 79). A refusal is a fact you are short of, not a
person you have not asked. The pack test suite asserts the sentence
byte-for-byte so it cannot quietly soften; § Gates the host can see, below,
is how you produce the fact.

## Worktrees, and the operator's live checkout

Every lane works in its own worktree under
`/Users/brian/Projects/beekeeper/beekeeper.worktrees/<lane>`. The main checkout
`/Users/brian/Projects/beekeeper/beekeeper` is **hot**: Brian's dev app runs
from it and `tauri dev` rebuilds on write. Never edit, rebase or switch
branches there — a seat that does is editing under a running app. A seat handed
that cwd stops and says so (ledger item 80a).

Before any `cargo`/`pnpm`/`just`/git-hook command, activate hermit **as its
own command**: `. ./bin/activate-hermit`, in the worktree, and nothing else on
the line. Your shell keeps the environment between commands, so it is done
once; a redirect bolted onto it later poisons every gate on that line (see
§ Gates the host can see). Do not rewrite hook commands to work around an
unconfigured `PATH`.

**Your nest is the worktree.** A seat runs with `HOME` set to the operator's
own home, so `~/.config`, `~/.cargo`, `~/.ssh` and the desktop app's data
directory are *Brian's*, not yours. Write nothing outside your worktree and
your seat's own state (live-run finding 73). Say the same in every brief you
write.

## Gates the host can see

The push gate reads kind 44246 gate rows the host **observed** — rows the
provider wrote by watching your tool calls — and nothing you *say* about a
gate. `bee sessions explain arm` defines the three arms it admits by. The
policy's required gates are, by default, `cargo fmt`, `cargo clippy` and
`cargo test` (any arguments), each observed green on the pushed commit over a
clean worktree; `pnpm test`, `pnpm typecheck`, `pnpm lint`, `just check`,
`just test` and `just ci` are also recognised.

**The host records a row only for a bare command.** A pipe (`|`), a redirect
(`>`, `2>`, `<`), a `$(…)` or backtick substitution, or a trailing `; echo` of
`$?` makes it record **nothing, silently** — the gate ran, the tool result
shows it, and no row exists (live-run finding 77; run 7's lead lost two hours
to it). A `&&`/`;` chain is split into segments, but one redirect anywhere
refuses the whole line, and a hermit activation with its output silenced
followed by `&& cargo fmt …` is exactly that line. A path to the program is
fine — `bin/cargo fmt …` records a row. Hermit off the `PATH` is not fine: Andy's seat's first gate was `cargo: command not
found`, and some seats' Bash tool refuses `. ./bin/activate-hermit` outright (finding 87) —
so run gates as `bin/cargo …`, which needs no activation. Run them AFTER `git commit`: a row
names HEAD at the moment it runs, and rows minted on the uncommitted tree name the previous
commit, dirty (finding 86).

The shape — one command per tool call, in the worktree, at the commit you
will push:

```
git commit -s -m "<what changed>"        # commit FIRST: a row names HEAD at the moment it runs (finding 86)
bin/cargo fmt --all --check                 # bin/cargo needs no activation; a path names the same gate (finding 80)
bin/cargo clippy -p buzz-cli -- -D warnings
bin/cargo test -p buzz-cli --lib
```

Read the exit code from the tool result — never by appending an `echo`.
**Scope the test to what changed**: `cargo test -p <touched crate> --lib` for
a small change. A doc-only lane that ran the whole workspace `cargo test`
spent seven minutes and hit an unrelated environment-sensitive test (live-run
finding 78); the pre-push hook runs the full floor anyway.

Before the push, ask the relay what it will do, and read the rows it will
fold:

```
bee git check --ref refs/heads/main                                  # `admitted by arm (B)` …, or the refusal
bee sessions observations --channel <uuid> --session-ref <uuid>     # every row, with headSha and dirty
```

### What a refusal says, and what you do

| the refusal says | what you do |
| --- | --- |
| gate `X` has no observed green row on `<sha>` | run gate X again as a bare command, in the worktree, at that commit |
| gate `X` was observed red on `<sha>` | fix, commit, run it again — the next row names the new commit |
| `<sha>` was observed dirty | modified or untracked files were present when the gate ran: commit or remove them, run again (a staged pack under `.agents/` is excluded automatically) |
| no approved report names `<sha>` … No observed gate row names `<sha>` either | arm (B) applies when the policy requires no verifier: produce the rows |
| no active verifier seat has cleared the report, with `verifierRequired` true | hire a verifier, or wait for its `not-refuted` refutation — and the rows are owed as well |
| this key holds no active seat | the seat lapsed: resume the session, then push again |

Every remedy is a command or a hire. **None is a person.** A refusal you
cannot clear is `bee pulse update --kind blocker` carrying the sentence
verbatim and the event ids of the rows you read — never a line to the founder
asking for a push.

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
public API. Each of those is run as a bare command, hermit activated first on
its own line (§ Gates the host can see), or the push gate never learns it ran. Desktop text uses rem tokens only (§ Text sizing & zoom); every
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
