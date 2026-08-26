#!/usr/bin/env bash
# scripts/crew/refute.sh — cross-family refuter wrapper around the Codex CLI.
#
# Runs one read-only Codex pass over a lane's diff and forces the result
# through scripts/crew/verdict.schema.json (the crew workflow's closed
# verdict vocabulary: CONFIRMED with a concrete file:line failure scenario,
# or NOT-REFUTED — never anything else). This is the crew plan's tier-2
# refuter role (docs/CREW_SESSIONS_PLAN.md §1), run with a different model
# family than whatever built the lane. See docs/CREW_REFUTE.md for the
# model-family rule and usage notes.
#
# Usage:
#   scripts/crew/refute.sh <worktree> <base-ref> <brief.md> [-m model]
#
#   <worktree>   path to the git worktree holding the lane's branch, checked out.
#   <base-ref>   ref (branch or sha) the lane branch diverged from. The diff
#                reviewed is `git diff <base-ref>...HEAD` inside <worktree>.
#   <brief.md>   path to the brief the lane was given. Its text is sent
#                verbatim as the constraints to refute against.
#   -m model     model to pass to `codex exec -m`. Omit to use Codex's
#                configured default. Pick a model from a different family
#                than the one that built the lane — see docs/CREW_REFUTE.md
#                § Model-family rule. When the families are the same, the
#                caller (not this script) must record the verdict as
#                "advisory" in the plan's §7 row.
#
# Behavior:
#   - If the diff is empty, no LLM call is made: this script deterministically
#     writes a NOT-REFUTED verdict ("nothing to refute") and exits 0. There is
#     nothing for a refuter to confirm against zero changes.
#   - Otherwise, runs `codex exec` with --sandbox read-only inside <worktree>,
#     forcing output through verdict.schema.json via --output-schema, and
#     validates the result (well-formed JSON, required fields, closed verdict
#     vocabulary, CONFIRMED implies at least one finding with file+line).
#
# Output: a verdict JSON file. The path is printed as the last line of
# stdout. Override the location with CREW_REFUTE_OUT=<path>; otherwise a
# file is created under ${TMPDIR:-/tmp}.
#
# Exit codes: 0 success. Non-zero on bad arguments, missing files, a codex
# failure, or a malformed/non-conforming verdict.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
schema="${script_dir}/verdict.schema.json"

usage() {
  echo "Usage: $(basename "$0") <worktree> <base-ref> <brief.md> [-m model]" >&2
}

if [ "$#" -lt 3 ]; then
  usage
  exit 1
fi

worktree="$1"
base_ref="$2"
brief="$3"
shift 3

model=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -m)
      if [ "$#" -lt 2 ]; then
        echo "refute.sh: -m requires a model argument" >&2
        exit 1
      fi
      model="$2"
      shift 2
      ;;
    *)
      echo "refute.sh: unrecognized argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [ ! -d "$worktree" ]; then
  echo "refute.sh: worktree not found: $worktree" >&2
  exit 1
fi
if [ ! -f "$brief" ]; then
  echo "refute.sh: brief not found: $brief" >&2
  exit 1
fi
if [ ! -f "$schema" ]; then
  echo "refute.sh: schema not found: $schema" >&2
  exit 1
fi
if ! command -v codex >/dev/null 2>&1; then
  echo "refute.sh: codex CLI not found on PATH" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "refute.sh: python3 not found on PATH (needed to validate the verdict)" >&2
  exit 1
fi

if ! git -C "$worktree" rev-parse --verify "$base_ref" >/dev/null 2>&1; then
  echo "refute.sh: base-ref does not resolve inside $worktree: $base_ref" >&2
  exit 1
fi

head_sha="$(git -C "$worktree" rev-parse HEAD)"
diff_range="${base_ref}...HEAD"
diff_text="$(git -C "$worktree" diff "$diff_range")"

if [ -n "${CREW_REFUTE_OUT:-}" ]; then
  out_file="$CREW_REFUTE_OUT"
else
  # A temp *directory* (not a suffixed template) because BSD mktemp (macOS)
  # does not honor X's followed by a literal suffix like GNU mktemp does —
  # it silently returns the template unexpanded instead of a unique path.
  out_dir="$(mktemp -d "${TMPDIR:-/tmp}/crew-refute.XXXXXX")"
  out_file="${out_dir}/verdict.json"
fi

# Empty diff: nothing to refute. Do not spend an LLM call confirming that
# zero changes contain zero defects — answer deterministically instead.
if [ -z "$diff_text" ]; then
  python3 - "$out_file" "$head_sha" <<'PY'
import json
import sys

out_file, head_sha = sys.argv[1], sys.argv[2]
verdict = {
    "lens": "empty diff (base-ref...HEAD has no changes) — nothing to refute",
    "verdict": "NOT-REFUTED",
    "reviewedSha": head_sha,
    "findings": [],
}
with open(out_file, "w") as f:
    json.dump(verdict, f, indent=2)
    f.write("\n")
PY
  echo "refute.sh: empty diff, skipped codex, wrote NOT-REFUTED verdict" >&2
  echo "$out_file"
  exit 0
fi

brief_text="$(cat "$brief")"

prompt="$(cat <<PROMPT
You are the refuter seat in a crew workflow (see docs/CREW_SESSIONS_PLAN.md
§1 if present in this worktree). Your job is one pass over a diff, checked
against the named constraints in the brief below. You do not redesign, you do not
re-argue a prior disposition, and you do not review anything outside this
diff.

Verdict vocabulary (closed — use exactly one of these two strings for the
"verdict" field, nothing else):
  - CONFIRMED: only when you can name a concrete failure — specific
    inputs/state that produce a wrong outcome or crash — anchored to a
    file:line in the diff. Put that scenario in a findings[] entry.
  - NOT-REFUTED: the diff does not violate the named constraints in the brief as
    far as you can tell from this review. This is the default; "I have
    concerns" without a concrete scenario is NOT-REFUTED, not CONFIRMED.

Output must conform exactly to the attached JSON schema (--output-schema).
Do not include any field not in the schema. If verdict is NOT-REFUTED,
findings must be an empty array. If verdict is CONFIRMED, findings must have
at least one entry with a real file path from the diff, a real line number,
and a scenario a reader could reproduce.

Set reviewedSha to: ${head_sha}
Set lens to a one-line statement of what you checked (the named constraints
in the brief, in your own words).

=== BRIEF (${brief}) ===
${brief_text}

=== DIFF (${diff_range} inside ${worktree}) ===
${diff_text}
PROMPT
)"

codex_args=(exec -C "$worktree" --sandbox read-only --skip-git-repo-check
  --output-schema "$schema" -o "$out_file" --json)
if [ -n "$model" ]; then
  codex_args+=(-m "$model")
fi
codex_args+=(-)

# No literal suffix after the X's — see the out_dir comment above for why.
codex_log="$(mktemp "${TMPDIR:-/tmp}/crew-refute-codex.XXXXXX")"
if ! printf '%s' "$prompt" | codex "${codex_args[@]}" >"$codex_log" 2>&1; then
  status=$?
  echo "refute.sh: codex exec failed (exit $status); log:" >&2
  cat "$codex_log" >&2
  rm -f "$codex_log"
  exit "$status"
fi
rm -f "$codex_log"

if [ ! -s "$out_file" ]; then
  echo "refute.sh: codex produced no output at $out_file" >&2
  exit 1
fi

# Validate: well-formed JSON, required fields present, closed vocabulary,
# CONFIRMED implies at least one findings entry with file+line.
if ! python3 - "$out_file" <<'PY'
import json
import sys

out_file = sys.argv[1]
try:
    with open(out_file) as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    print(f"refute.sh: verdict is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(1)

required = ["lens", "verdict", "reviewedSha", "findings"]
missing = [k for k in required if k not in data]
if missing:
    print(f"refute.sh: verdict missing required fields: {missing}", file=sys.stderr)
    sys.exit(1)

if data["verdict"] not in ("CONFIRMED", "NOT-REFUTED"):
    print(
        f"refute.sh: verdict field is not in the closed vocabulary: {data['verdict']!r}",
        file=sys.stderr,
    )
    sys.exit(1)

findings = data["findings"]
if not isinstance(findings, list):
    print("refute.sh: findings is not an array", file=sys.stderr)
    sys.exit(1)

finding_fields = ["id", "file", "line", "severity", "scenario", "fix"]
for i, item in enumerate(findings):
    if not isinstance(item, dict):
        print(f"refute.sh: findings[{i}] is not an object", file=sys.stderr)
        sys.exit(1)
    missing_f = [k for k in finding_fields if k not in item]
    if missing_f:
        print(f"refute.sh: findings[{i}] missing fields: {missing_f}", file=sys.stderr)
        sys.exit(1)

if data["verdict"] == "CONFIRMED" and len(findings) == 0:
    print(
        "refute.sh: verdict is CONFIRMED but findings is empty — "
        "CONFIRMED requires a concrete failure scenario with file:line",
        file=sys.stderr,
    )
    sys.exit(1)

sys.exit(0)
PY
then
  exit 1
fi

echo "refute.sh: verdict written to $out_file" >&2
echo "$out_file"
