# Project team setup: local workbench and Solo compatibility

Owner: Astra. Worktree: `work/project-team-setup-astra`, based on `9aebb1262`.
This is an implementation checkpoint, not a landing or installed-build claim.
The governing plan is [`PROJECT_TEAM_SETUP_IMPL.md`](../PROJECT_TEAM_SETUP_IMPL.md).

## Implemented

- Shipped role packs now provide project-neutral procedures. Existing seven
  role IDs remain; `project-setup` is the eighth. The separately published
  Beekeeper pack repository is unchanged. Project-specific setup remains opt-in.
- Roles has a local setup workbench: project intent, a checked repository root,
  preserved draft, real pack/skill validation and a readable authoring brief.
  Merely opening it reads state. Preparing never changes the project source,
  installs identities, publishes events or launches a model.
- Native drafts are scoped by community, signing identity and project. Matching
  retries retain edits; mismatched intent/folder refuses to overwrite them.
  Validation covers actual loader results, skill metadata, role identity,
  bounded content and path containment. The optional team needs a lead; Solo
  does not. The roster can remove optional baseline roles or add project roles.
- Switching Team to Solo no longer leaves hidden role-readiness or missing
  bench-identity errors blocking Start. Runtime, authentication, workspace and
  prompt checks remain active; switching back to Team retains its draft.

## Verification before the snapshot/reservation extension

Commands ran in the isolated worktree on 2026-09-11:

| Check | Result |
| --- | --- |
| `cargo test -p buzz-persona` | 188 passed; actual shipped role/skill and neutral-content checks included |
| Native setup tests | 12 passed, including the actual neutral seed; real eight-agent installer test also passed |
| Focused Solo readiness/start tests | 22 passed after the Team-to-Solo fix |
| `just desktop-check` | passed; 3 existing warnings and 6 infos |
| `just desktop-test` | 9,258 passed, 0 failed or skipped, across 85 suites |
| `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` | passed |
| Founded Solo browser regression | failed Team readiness plus missing bench identity, then Solo start without actor/role/policy/grants: passed |
| Setup browser tests | 2 passed; form and draft checked at 640px and 250% text size |

Raw desktop logs and four visually inspected screenshots are under
`../review-project-team-setup-astra-logs/`. These are local evidence, not tracked
artifacts available to another developer. The executable tests are tracked in
the candidate. Mock-bridge browser results are not native installed acceptance.

## Review findings resolved

- Reopening edited drafts no longer implies their contents remain untouched
  neutral defaults.
- Validation now checks every effective shared skill's metadata, including
  skills not explicitly named by a persona.
- Real shipped-role installer expectations include the eighth role; the
  existing four default seats and synthetic seven-role fixture are unchanged.
- Team-only readiness does not escape into Solo.

## Remaining work

The extension now saves and re-verifies exact checked draft versions and
reserves authoring IDs plus a signed genesis durably. Native setup tests passed
37/37: 12 draft tests, 14 snapshot tests and 11 authoring reservation tests.
The snapshot manifest detects changed bytes and added files; Windows-invalid
names and case collisions are rejected. A durable latest-version pointer lets
reopening reverify the saved copy while leaving the editable draft unchecked.
On Unix, containing directories are flushed along with files; native Windows
directory durability remains unverified.

Authoring reservations have a separate owner-signed, domain-separated journal
seal binding IDs, project, owner, community and setup. Tests reject altered
command IDs and rewritten same-owner scope metadata. The exact signed genesis
is stored before return, with a cross-process lock preventing duplicate
reservations. No channel publication, identity minting or execution occurs.

Frontend follow-up: 9/9 focused setup tests; 5/5 setup browser cases, then a
fresh-build saved-reopen regression 1/1. Saved-version details were visually
checked at 640px and 250% text size. Independent review cleared snapshot
portability/persistence and reservation integrity findings. Neither operation
proves a published or running session. The visible authoring launch still needs a real
setup identity, exact shipped-pack staging and receipt-based reconciliation.
It must verify that the selected channel belongs to the intended project
before publishing the saved genesis; a reserved UUID alone does not prove that.
Automatic pack publication, project-qualified team installation, lead handoff
and live Tankloop acceptance remain unimplemented.

The publication trace found that the existing initializer cannot be reused as
an idempotent coordinator: an ambiguous push error can trigger announcement
cleanup. Project source kind 30624 also lacks atomic expected-source checking.
Those contracts must be implemented and tested before promising safe
concurrent publication. See the plan for the exact boundary.

Final native gate: `cargo test --manifest-path desktop/src-tauri/Cargo.toml
--lib` passed 3,222 tests, with 18 existing ignored tests. Its first run found
one additional seven-role inventory assertion in `packs_cache.rs`; the
assertion now includes `project-setup`, and the entire suite passed on rerun.
Final Tauri `clippy --all-targets -- -D warnings`, Rust format checks,
`just desktop-check`, file-size checks and current-state size check passed.
The full repository `just ci` and release/native installed acceptance were not
run for this local milestone candidate. Final logs are under the local evidence
directory named above (`tauri-test-final.log`, `desktop-check-final.log`).

Nothing in this checkpoint was pushed, installed or applied to a live project.
