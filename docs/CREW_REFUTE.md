# Crew tier-2 refute — the Codex wrapper

`scripts/crew/refute.sh` runs one read-only refute pass over a lane's diff
using the locally installed Codex CLI, and forces the result through
`scripts/crew/verdict.schema.json`. It exists so the crew workflow's tier-2
refuter (`docs/CREW_SESSIONS_PLAN.md` §1: "one pass over a tier-2 diff
against the brief's named constraints; terminal verdict") can run with a
model family different from whatever built the lane, without a human
re-typing the diff and brief into a chat window each time.

This tool is tier-0/1 itself: it is scripts and docs, touches no runtime
code, and makes no product decisions. It does not decide APPROVE/BLOCK —
that stays the lead's call, made by reading the verdict this produces plus
the diff, per §1 of the plan.

## Usage

```
scripts/crew/refute.sh <worktree> <base-ref> <brief.md> [-m model]
```

- `<worktree>` — path to the git worktree holding the lane's branch, checked out.
- `<base-ref>` — the ref (branch or sha) the lane branch diverged from. The
  reviewed diff is `git diff <base-ref>...HEAD` inside `<worktree>`.
- `<brief.md>` — path to the brief the lane was given (the lead's §1.1
  template). Its full text is sent to the model as the constraints to check
  the diff against.
- `-m model` — optional, passed through to `codex exec -m`. Omit to use
  Codex's configured default model. See § Model-family rule below for how to
  choose this.

The verdict JSON path is printed as the last line of stdout (diagnostics go
to stderr). Override the output location with `CREW_REFUTE_OUT=<path>`;
otherwise a fresh temp directory is created per run
(`${TMPDIR:-/tmp}/crew-refute.XXXXXX/verdict.json`) and never reused, so
concurrent runs cannot clobber each other's output.

Exit codes: `0` on a valid verdict (either `CONFIRMED` or `NOT-REFUTED`).
Non-zero on bad arguments, a missing worktree/brief/schema, a `codex exec`
failure (the underlying exit code is propagated), or a verdict that fails
validation (not JSON, missing a required field, a `verdict` value outside
the closed vocabulary, or `CONFIRMED` with an empty `findings` array).

### Example

```
scripts/crew/refute.sh \
  /Users/brian/Projects/beekeeper/beekeeper.worktrees/lane-s2a \
  main \
  /tmp/lane-2a-brief.md \
  -m gpt-5.6-terra
```

### Empty-diff behavior

If `<base-ref>...HEAD` is empty (the lane made no changes relative to its
base), the script does **not** call Codex at all — there is nothing for a
refuter to confirm against zero changes, and spending a model call to say so
would be theater. It deterministically writes a `NOT-REFUTED` verdict
(`lens: "empty diff ... — nothing to refute"`, `findings: []`,
`reviewedSha` set to the worktree's actual `HEAD`) and exits `0`. This is
also how the tool is dry-run tested — see § Verified below.

## The verdict schema

`scripts/crew/verdict.schema.json` defines the closed shape, field names
identical to the crew workflow's `VERDICT_SCHEMA`
(`docs/CREW_SESSIONS_PLAN.md` §1):

```
{
  "lens": string,        // one-line statement of what was checked
  "verdict": "CONFIRMED" | "NOT-REFUTED",
  "reviewedSha": string, // the worktree HEAD sha this verdict covers
  "findings": [
    { "id", "file", "line", "severity" (blocker|major|minor), "scenario", "fix" }
  ]
}
```

Every object in the schema sets `additionalProperties: false` and lists
every property in `required`, and it avoids `pattern` / `minimum` /
`minItems` / `format` — Codex's `--output-schema` structured-output mode
(OpenAI's strict-schema subset) does not support those keywords, and a
schema that uses them either gets silently loosened or rejected depending on
the provider. Keep any future edits to this schema inside that subset.

**Verdict vocabulary is closed, per the plan:** `CONFIRMED: <inputs/state →
wrong outcome>` requires a `findings[]` entry with a real `file`/`line` and a
concrete, reproducible `scenario` — otherwise the result is `NOT-REFUTED`.
"I have concerns" is not a verdict; `refute.sh` rejects a `CONFIRMED` verdict
whose `findings` array is empty (exit non-zero) rather than passing it
through.

## Model-family rule

The refuter must be a **different model family** from whatever built the
lane, per `docs/CREW_SESSIONS_PLAN.md` §1 (the refuter row: "a *different
model family* from the builder") and the open question in §6.2 (a hard
launch refusal vs. advisory — this wrapper does not enforce either policy
programmatically; it is a manual choice at call time). `refute.sh` never
inspects who built the lane, so the caller is responsible for the check:

- **Builder was a Claude/Anthropic model (Sonnet/Opus 5, this crew's usual
  builder family):** run `refute.sh` with Codex's default model (currently
  `gpt-5.6-terra`, an OpenAI model — see § Verified below) or pass `-m` with
  another non-Anthropic model. This is a genuine cross-family refute.
- **Builder was itself a Codex/OpenAI model:** running this wrapper checks
  the diff with the *same* family as the builder. Record the resulting
  verdict as **`advisory`** in the plan's §7 row (e.g. `refuted NOT-REFUTED
  (advisory, same family) ...`), not as a full tier-2 refute. Same-family
  review still catches real bugs, but it does not satisfy the plan's
  cross-family independence requirement, and the ledger entry must say so
  honestly rather than implying it did.

## Where the verdict goes

`docs/CREW_SESSIONS_PLAN.md` §7 is the ledger. After a run, copy the
verdict's `verdict` value, a one-line gist of `lens`, and (if `CONFIRMED`)
each finding's `id`/`file:line`/`scenario` into that slice's row, using the
status vocabulary already defined there: `refuted NOT-REFUTED|CONFIRMED
<what>`. Prefix with `(advisory)` per § Model-family rule above when the
refuter ran same-family. The lead reads the verdict JSON and the diff, not
this tool's stderr log or the raw Codex transcript.

A `CONFIRMED` verdict is a report back to the builder lane, not an
authorization to fix it yourself in this lane — this tool only produces the
verdict; routing the fix is the lead's job per §1's role table.

## Facts this doc pins (verified 2026-08-26)

- `codex` is `codex-cli 0.148.0` at `/Users/brian/.local/bin/codex`, logged
  in via ChatGPT. `codex exec --help` confirms the flags this wrapper relies
  on: `-C <dir>`, `-s/--sandbox read-only`, `-m <model>`, `--ephemeral`,
  `--output-schema <file>`, `-o <file>` (last message), `--json`, and a
  `-`/stdin prompt source ("If not provided as an argument (or if `-` is
  used), instructions are read from stdin").
- **Smoke test** (the only other permitted Codex invocation while building
  this tool, run read-only + ephemeral against a scratch directory, prompt
  `"Reply with the single word ok"`):

  ```
  $ codex exec -C /tmp/codex-smoke --sandbox read-only --ephemeral \
      --skip-git-repo-check "Reply with the single word ok"
  OpenAI Codex v0.148.0
  --------
  workdir: /tmp/codex-smoke
  model: gpt-5.6-terra
  provider: openai
  approval: never
  sandbox: read-only
  reasoning effort: low
  reasoning summaries: none
  --------
  user
  Reply with the single word ok
  codex
  ok
  tokens used
  5,001
  ```

  Exit code `0`, final line `ok`, reproducible on a second run. This is what
  pinned the default model as `gpt-5.6-terra` (OpenAI/GPT family) — the
  fact behind the "run with Codex's default to get a cross-family refute
  against a Claude builder" guidance above.

## Verified

- `bash -n scripts/crew/refute.sh` — clean.
- `shellcheck` is not installed on this host (`which shellcheck` → not
  found), so no shellcheck run is recorded here. The script avoids the
  common shellcheck complaints by construction (`set -euo pipefail`,
  quoted expansions throughout, no unquoted globs).
- Dry run against an empty diff (`scripts/crew/refute.sh . HEAD
  /tmp/dummy-brief.md` from inside this worktree, `base-ref=HEAD` so the
  diff is empty by construction regardless of branch state):

  ```
  $ bash scripts/crew/refute.sh . HEAD /tmp/dummy-brief.md
  refute.sh: empty diff, skipped codex, wrote NOT-REFUTED verdict
  /var/folders/.../crew-refute.Ib0SLG/verdict.json
  $ cat /var/folders/.../crew-refute.Ib0SLG/verdict.json
  {
    "lens": "empty diff (base-ref...HEAD has no changes) — nothing to refute",
    "verdict": "NOT-REFUTED",
    "reviewedSha": "b2298102abad0c40cf149d16b357b65db39378d5",
    "findings": []
  }
  ```

  Exit code `0`.
- Argument validation, exercised directly (each exits `1` with a message on
  stderr, no verdict file written): missing brief file, missing worktree
  directory, an unrecognized flag, fewer than 3 positional arguments, and a
  `base-ref` that does not resolve inside the worktree.
- The malformed-output validator (the same Python block `refute.sh` runs
  against whatever Codex writes) was exercised standalone against six fixed
  JSON payloads: a valid `NOT-REFUTED`, a valid `CONFIRMED` with one
  well-formed finding, a payload missing `reviewedSha`, a `verdict` value
  outside `{CONFIRMED, NOT-REFUTED}`, a `CONFIRMED` with an empty
  `findings` array, and a non-JSON payload. The two valid payloads exited
  `0`; all four malformed payloads exited `1` with a specific stderr
  message naming what was wrong.
- The one-line Codex smoke test above (`--ephemeral`, `--sandbox
  read-only`, throwaway `/tmp` directory) is the only Codex invocation with
  real content this lane made; the actual `codex exec ... --output-schema
  ...` call path inside `refute.sh` (the non-empty-diff branch) was written
  and syntax-checked but not exercised end-to-end in this lane, per the
  brief's restriction on Codex usage. The first live tier-2 lane that calls
  this wrapper is the real end-to-end proof of that path.
