# Project team publication — conditional source checkpoint

2026-09-12. Astra. Topic: `work/project-team-setup-astra`, following
`048dc4e02`. This report distinguishes source changes from executed evidence.

## What this slice is for

A setup agent can already author an isolated team draft and the host can save
a checked snapshot. The next step is publishing that snapshot as the project's
shared role source. Before automating that step, a competing source change
must produce a conflict instead of being silently overwritten.

The implementation contract is milestone 2 of
[`PROJECT_TEAM_SETUP_IMPL.md`](../PROJECT_TEAM_SETUP_IMPL.md). This slice adds
the conditional source operation, not the full host publication coordinator,
project roster installation or lead handoff. Ordinary Solo remains independent.

## Changes under review

- Kind 30624 retains unconditional v1 and adds v2 with a required
  `expectedSourceId`: explicit null means no live source; a lowercase event ID
  means that exact effective source. Unknown or incomplete conditions refuse.
- Storage serializes source writes across authors for one community/project.
  Conditional writes compare the effective source inside the transaction;
  same-second ordering uses timestamp descending, then event ID ascending.
  Legacy writes and source deletions participate in the same project lock.
- A retained signed event is a retry, even after replacement or deletion. It
  does not switch the source back. Community lifecycle fencing remains in force.
- HTTP conflict is 409; WebSocket refusal carries `PACK_SOURCE_CONFLICT`;
  `bee packs set-source` exposes `--if-unset` and `--expected-source` and uses
  conflict exit 5. Existing unconditional callers remain supported.
- The desktop reader understands v2 and the same winner ordering. The ordinary
  desktop publisher remains v1; automatic setup publication is not enabled.

The code and test files, rather than this list, determine which parts are
implemented. No passing gate, installed build or production deployment is
claimed by this checkpoint.

## Review constraints

The community deletion lock precedes the project lock and row mutations.
Existing deletion-executor authorization must remain usable. All production
source insertion APIs must route through the conditional transaction or refuse
an unsupported bypass. Deletion keeps its author and timestamp scope.

V2 requires a canonical project coordinate. Legacy aliases preserve their raw
NIP-33 coordinates and remain subject to existing exact `#d` query behavior;
this slice does not silently migrate or merge them.

Source comparison protects adoption, not an earlier Git push. The future host
publisher must push an isolated candidate ref, verify the commit and adopt that
immutable revision. It must preserve exact signed publication bytes before
sending and reconcile uncertain outcomes; admission can reject an otherwise
identical retry outside its timestamp window. A new signature is not recovery.

## Verification state

Opus's steering smoke completed before these executable checks began.
All following runs used this topic's source, before integration with steering.
Logs are under `../review-project-team-setup-astra-logs/` (local only).

| Check | Result | Log |
| --- | --- | --- |
| Core source decoder/builder | 14 passed | `conditional-core.log` |
| Desktop source reader | 22 passed | direct Node test output |
| CLI packs argument, publication and existing packs tests | 12 passed | `conditional-cli.log` |
| Scratch-Postgres source transactions | 3 passed | `conditional-db.log` |
| Relay source admission and conditional publication | 5 passed | `conditional-relay.log` |
| Conformance checker/unit/property tests | 22 passed | `conditional-conformance.log` |
| TypeScript typecheck | exit 0 | `conditional-tsc.log` |
| Clippy, core/database/CLI/relay/conformance, all targets | exit 0 | `conditional-clippy.log` |
| Rust formatting, scoped Biome, whitespace and size ratchets | passed | command output / `conditional-fmt.log` |

Independent source review found a desktop mismatch: Rust accepted and
normalized a v2 SHA that the desktop reader could hide. V2 normalization and
regression cases fixed it. Duplicate JSON fields also refuse, including an
ambiguous schema overwritten to v1. The new conflict category required an
explicit conformance error-alphabet update; the checker and property tests
include it. The TLA configuration names it, but no new TLC run is claimed.

Full `just ci`, broad integration and the combined steering build remain
pending. No installed build, live publication or Tankloop setup acceptance is
claimed. The conditional operation is available to the CLI; connecting it to
the host's durable snapshot-publication journal is still milestone 2 work.

## Repository instructions audit

Both Solo and a role seat pass their resolved working directory to the coding
session adapter (`crates/buzz-session-provider/src/session.rs`,
`session_new_full`). The role body and materialized skill names are appended
by `session_briefing`; Solo has no role body. This path does not use
`crates/buzz-acp/src/base_prompt.md`, so that file's AGENTS reminder does not
prove instruction delivery for coding sessions.

Read-only inspection of locally installed adapters found:

- Claude ACP 0.70.0 keeps its `claude_code` preset for Beekeeper's
  `{append: briefing}` system-prompt form and enables user, project and local
  settings. Its bundled SDK documents the project setting as necessary for
  loading CLAUDE.md. This supports additive role instructions and normal
  Claude repository discovery.
- Codex ACP 1.6.2 forwards CWD/configuration to native `thread/start`; the
  installed adapter advertises protocol 1, so Beekeeper supplies its briefing
  as a first-turn preamble. Native AGENTS discovery was not proved by that
  adapter-source inspection.

These packages were read under Beekeeper's local `node-tools/lib/node_modules/`
directory. This is local source evidence, not a model execution. Beekeeper's
own `CLAUDE.md` symlink to `AGENTS.md` masks the distinction between those files.

Live acceptance should use isolated AGENTS-only, CLAUDE-only and combined
fixtures with harmless, distinct markers. Compare Solo before/after team setup
and a role seat, recording CWD, adapter versions, file hashes and the seat's
signed packRef. Do not put marker contents in the task prompt. Existing
Tankloop instruction files must not be rewritten as a side effect of setup.
