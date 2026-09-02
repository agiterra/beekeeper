---
name: push-your-lane
description: "How a verifier pushes its lane branch without the pre-push hooks timing the tool out or expiring the relay credential."
---

# Push your lane

The repository's pre-push hooks run the whole gate. On a seat that means two
things go wrong at once: the push outruns the 60 s tool timeout, and the NIP-98
credential git minted at ref discovery expires inside the hook window, so the
relay answers HTTP 401 on a push whose hooks were green
(`docs/INTEGRATION.md` § "Landing a batch").

The budget is not the problem and is not raised — the hooks are what make a
green SHA green. Run them yourself, deliberately, and then push the SHA they
saw:

```
Run the floor first, on the exact SHA you are about to push: cargo fmt --check, cargo clippy --all-targets -- -D warnings and the unit tests for every crate you touched, plus pnpm typecheck && pnpm test if you touched desktop, plus anything else your brief names. Only then push with the hooks skipped, on that identical SHA: git push --no-verify origin <branch>. Never --no-verify on a SHA no gate has run against, and never push to GitHub directly: origin is the relay and the bridge mirrors it.
```

## The floor is a floor

Those four commands are the minimum, whatever your brief says. A brief that
names one test suite does not shrink them: skipping the hooks is only honest
because an equivalent gate already ran on those exact bytes, and a SHA that no
clippy, no typecheck and no fmt has seen is not one you may push with
`--no-verify`. Your brief will usually add the gates you were asked to re-run against the lane under review; that is on top of the floor, never instead of it.

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
