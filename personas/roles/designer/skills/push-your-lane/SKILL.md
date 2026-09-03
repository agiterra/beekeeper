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
  the gate's own summary lines are.
