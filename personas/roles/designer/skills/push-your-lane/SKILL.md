---
name: push-your-lane
description: "How a designer pushes its lane branch without the pre-push hooks timing the tool out or expiring the relay credential."
---

# Push your lane

The repository's pre-push hooks used to run the whole gate. On a seat that made
two things go wrong at once: the push outran the 60 s tool timeout, and the
NIP-98 credential git minted at ref discovery expired inside the hook window,
so the relay answered HTTP 401 on a push whose hooks were green
(`docs/INTEGRATION.md` § "Landing a batch").

The floor is now **scoped to what you changed** and prints what it skipped, so
it fits inside the budget on a normal lane. That means the answer is no longer
"run the gate yourself and then skip the hooks" — it is simply: run what your
brief asks for, then push, and let the floor run.

```
Run your own gate first — the one your brief names. Then push normally: git push origin <branch>. The pre-push floor is scoped to what you changed and prints what it skipped; do not pass --no-verify, and never push to GitHub directly: origin is the relay and the bridge mirrors it.
```

## The floor is a floor

The floor is the minimum, whatever your brief says, and it runs on the push —
you do not run it and you cannot shrink it. A brief that names one test suite
adds to it. `--no-verify` is what turns that guarantee off, which is why the
rule above says not to pass it: a SHA no clippy, no typecheck and no fmt has
seen is not one you may push. Your brief will usually add a screenshot pass and `pnpm check:px-text`; that is on top of the floor, never instead of it. If you touched `desktop/` at all, `pnpm typecheck && pnpm test` is part of the floor, not an extra.

If you have time for the real thing, `just check` is the whole pre-push gate
and is strictly better than the floor.

## Gates the host can see

The relay's push gate reads kind 44246 rows the host **observed** — rows the
provider wrote by watching your tool calls — never what your report says
about a gate. It records a row **only for a bare command**: a pipe (`|`), a
redirect (`>`, `2>`, `<`), a `$(…)` or backtick substitution, or a trailing
`; echo` of `$?` makes it record nothing, silently (live-run finding 77; a
lead lost two hours to it in run 7). A `&&`/`;` chain is split into segments,
but one redirect anywhere refuses the whole line — a hermit activation with
its output silenced, followed by `&& cargo fmt …`, is exactly that line. A
path to the program is fine (`bin/cargo fmt …` records a row); hermit off the
`PATH` is not — Andy's seat's first gate was `cargo: command not found`.

The shape — one command per tool call, in your worktree, at the commit you
will push:

```
. ./bin/activate-hermit                     # first, alone; your shell keeps it
cargo fmt --all --check
cargo clippy -p <crate you touched> -- -D warnings
pnpm typecheck
pnpm test
```

Read the exit code from the tool result — never by appending an `echo`.
`pnpm typecheck` and `pnpm test` are the floor when you touched `desktop/`; each is its own bare command, not one `&&` line with a redirect in it. The required gates are, by default, `cargo fmt`, `cargo clippy`
and `cargo test` (any arguments), each observed green on the pushed commit
over a clean worktree; `pnpm test`, `pnpm typecheck`, `pnpm lint` and the
`just` gates are recognised too. `bee sessions observations --channel <uuid>
--session-ref <uuid>` lists the rows with their `headSha` and `dirty`.

If a landing is yours to push, `bee git check --ref refs/heads/main` prints
what the relay will do first (`admitted by arm (B)` …, or the refusal), and
`bee sessions explain arm` defines the arms. A refusal names the missing
fact, and every remedy is a command:

| the refusal says | what you do |
| --- | --- |
| gate `X` has no observed green row on `<sha>` | run gate X again, bare, at that commit |
| gate `X` was observed red on `<sha>` | fix, commit, run it again |
| `<sha>` was observed dirty | commit or remove the modified/untracked files, run again (`.agents/` is excluded already) |
| no approved report names `<sha>` … No observed gate row names `<sha>` either | produce the rows — arm (B) needs no verifier when the policy requires none |
| no active verifier seat has cleared the report, `verifierRequired` true | tell the lead: it hires a verifier, or waits for its `not-refuted` |
| this key holds no active seat | your seat lapsed: resume the session |

**None of them is a person.** Never end a report by asking a founder to push
the commit; a refusal you cannot clear goes on the wire as a blocker quoting
the sentence verbatim with the event ids of the rows you read (live-run
findings 75 and 79).

## Your nest is the worktree

A seat runs with `HOME` set to the operator's own home. Write nothing outside
your worktree and your seat's own state — not `~/.config`, not `~/.cargo`,
not the desktop app's data directory (live-run finding 73).

## The rest of it

- Do not amend, rebase or `git add` anything after the gate runs. A push is
  allowed to skip the hooks only because a gate already ran on **those exact
  bytes**; one more commit and that is no longer true, so run the gate again.
- `origin` is the relay (`hive.agiterra.org`). The forge's bridge mirrors
  every ref to GitHub within seconds, and GitHub triggers CI. Pushing to GitHub
  yourself races the bridge.
- Never hard-code a remote name anywhere else: run `git remote -v` and read
  it. The names moved on 2026-08-24 and two guards that hard-coded one broke
  silently that day.
- If the push still returns HTTP 401, that is the expired-credential case, not
  a broken key. Retry once on the identical SHA. If it fails again, stop and
  say so in your report rather than changing the SHA to make it pass.
- Paste the floor's output in your report. A completion report is not evidence;
  the gate's own summary lines are — and the observed rows are what the push
  gate reads.
