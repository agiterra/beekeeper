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

## Snapshot/reservation checkpoint, before the authoring connection

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

## Authoring connection — 2026-09-11

The workbench now starts a visible coding session for its draft. Opening the
controls only probes local runtimes/models and reads the preserved reservation
and launch. Explicit Start may provision the existing local provider, then
reserves the session, verifies the signed project/channel relationship,
prepares one standalone setup identity and stages the exact shipped bootstrap.
No lead or global team is installed as a prerequisite, and Solo stays separate.

The actor's encrypted recovery receipt precedes managed-agent/keyring writes.
Its shipped pack bytes and original packRef are preserved outside the editable
draft. The complete signed create, runtime/model/provider-instance binding and
owner seal are stored before external effects; an event marker prevents a
missing launch journal from silently producing different signed bytes. The
provider and actor join the selected channel through the existing membership
path, with readback. Genesis and create retries retain their exact bytes.
Verified receipts distinguish creation, failed first turn, refusal and unknown
delivery. A saved create is not a claim about the process's present health.

`PROJECT_TEAM_SETUP.md` carries the local repository and draft paths. The relay
request only asks the agent to read that file and author the draft; intent and
absolute paths are not placed in the create event. The ordinary provider loads
the pinned role and materializes its skills. The workbench links to the normal
session page once the provider returns its target. Starting authoring clears
any “current draft checked” display; an independently saved snapshot remains a
statement about its own immutable bytes.

Review closed three material integration findings:

- A sibling authoring directory put the draft outside Claude's existing write
  fence. The execution now runs at the draft root; private custody/bootstrap
  remain siblings outside it. The host's emitted policy permits repository
  reads. This does not claim Bash is a filesystem sandbox or that a live SDK
  enforced every instruction.
- A changed provider instance behind the same pubkey would never match the
  saved receipt target. Retry now rejects that change before publication.
- Per-draft launch locking did not protect the global directory store. All
  desktop writers and provider-view rematerialization now share an OS lock;
  twelve concurrent hint writes preserve one another and existing defaults.
  This also fixes the ordinary worktree writers (ledger finding 111).

Supervisor recovery resolves setup identities through their preserved
bootstrap before looking up the project's evolving role source. Generic
operator staging refuses these scoped identities rather than silently
substituting project packs. Explicit operator-driven resume through that
ordinary staging IPC is therefore still unsupported; retrying the authoring
request is not a replacement for resuming an already-created execution.

The browser suite covers read-only opening, explicit provisioning, uncertain
response recovery, exact retries, reopening without relaunch, recoverable
runtime discovery failure, saved snapshots and usable controls at 640px and
250% text size. The provider fixture uses the real SessionManager and a Bash
ACP child: it receives the correct cwd and role instructions, reads the local
brief and external repository, sees the materialized shipped skill, and writes
an artifact inside the draft. A separate real fence-layout test reproduces the
old sibling-directory denial and verifies the corrected boundary.

Named limits: no complete native coordinator + live relay + real model run;
no model-generated pack-quality acceptance; no native Windows acceptance;
no automatic channel creation for a project with no usable channel yet;
no pack publication, project-team installation or lead handoff. Provider
startup backfill is bounded to ten minutes when no consumed-command watermark
exists. Replaying an already accepted event is not guaranteed delivery after
that window because relay duplicates do not fan out; missing receipts stay
unknown and never justify creating a second conversation.

The broad checks caught two test/integration issues. The reusable form imported
the application's community wrapper, eventually reaching `mediaUrl.ts`'s eager
relay/proxy discovery even without rendering the wrapper. Under full-suite load
those delayed calls arrived during the snapshot test. An import-only subprocess
reproduced the unexpected reads (`setup-import-sideeffects.log`); separating the
form from the wrapper is the correction, with the no-publication allowlist kept
strict. Authoring itself loads lazily only for a prepared draft. The new HTTP
test server's incoming events route also needed a test-only entry in the egress
inventory. Production publication still uses the guarded submit funnel.

The native suite passed 3,250 tests with 18 existing ignored tests
(`authoring-tauri-final.log`). The real provider subprocess/fence checks passed
2/2 (`authoring-provider-subprocess.log`), focused setup checks passed 15/15,
and the fresh-build browser suite passed 7/7 (`authoring-browser-final.log`).
Six distinct narrow/250% captures are under `authoring-screenshots/` in the
local evidence directory. These are mock-bridge images, not installed-app proof.
Tauri all-target clippy, Rust format checks, desktop check and file-size checks
passed. After separating the form imports, the fresh full desktop suite passed
9,267 tests with zero failures (`setup-form-final-unit.log`), including the new
zero-IPC import regression. Provider all-target clippy also passed
(`authoring-provider-clippy.log`).
The final split-source browser rebuild and all seven setup cases passed again
(`setup-form-final-build.log`, `setup-form-final-browser.log`); TypeScript and
the full desktop check passed on the same source. Both Rust format checks and
the file-size gates passed after formatting the test-only egress inventory row.

### Publication prerequisite discovered during review

Two independent source traces found that a repository self-backlink could
qualify its author or maintainers to change a project's role source (ledger
112). The backlink only needed project read access, while kind 30624 treated
it as project pack authority. This includes public projects and private-project
Viewer members; NIP-OA owner attestation does not itself supply private-project
membership. No live exploit was attempted. A narrow local correction requires
the repository to appear in the project creator's signed forward roster before
its founders qualify. Creator and project Owner rights remain separate. The
production-query test passed against a newly migrated throwaway Postgres
database: endorsed maintainer access, community separation, removal, rejection
of an older endorsement, restoration and deletion. It now participates in
`just test-git-push-gate`, which `just test` invokes. Existing backlink-only
founders need creator endorsement before their next source change; installed
relay behavior is unchanged until deployment. The separate check-then-store
revocation race is unchanged and belongs to the transactional publication work.
This corrects admission of new writes; it does not delete or reclassify stored
source records, and no live inventory of existing records was performed.
Authority validation: eight normal unit tests, both ignored Postgres cases and
relay all-target clippy passed (`pack-source-unit-result.log`,
`pack-source-postgres-all.log`, `pack-source-relay-clippy.log`). Both exact
scratch database names were verified absent afterward. The correction received
an independent source review before finalization.

The first `just test` run passed the scratch genesis/authority, push admission,
CI completion and integration legs, but failed the workspace CLI unit suite.
The neutral role rewrite preserved a generic canonical-operation instruction
while dropping its exact response-field name. All eight personas now require
`operations[0].canonical` to be `true`. The content guard accepts the
host-selected `$BEE` spelling and ordinary whitespace wrapping, while retaining
the canonical-field assertion. The targeted CLI test and all 188 persona tests
passed. The CLI test file shrank by two lines. This is a local candidate
regression caught before publication, not an inherited failure. The full
`just test` rerun is on the corrected source and includes both new admission
filter cases in its scratch-database recipe.
The corrected persona bytes also passed all 63 native setup tests and both
shared-store lock tests (`persona-final-native-setup.log`,
`persona-final-native-workdir-lock.log`), including preparation from the actual
shipped baseline and preserved-bootstrap restaging/materialization.

Final `just test` passed on the corrected source (exit 0,
`setup-final-integration.log`): scratch genesis/authority 25, admission 70
(including both pack-source database cases), CI completion 3 + 6, and the
workspace unit/integration legs. The workspace phase took 418 seconds and
included the provider's 5,121-source pressure test and the new authoring child
fixture. This is not `just ci`, the whole browser smoke suite or native installed
acceptance; those broader landing/release gates remain owed before shipping.

The authority correction is the separate signed local commit `0f70d7633`.
The authoring checkpoint follows it on `work/project-team-setup-astra`.
Nothing from this checkpoint was pushed, installed, applied to a live project
or deployed to the relay.

The next publication contract is recorded in `PROJECT_TEAM_SETUP_IMPL.md`:
conditional source writes serialized across authors and deletion, durable exact
events, and a new Git candidate ref before adoption. Pushing an already adopted
moving ref would change team bytes before source comparison, so source-event
comparison alone cannot make that path safe. Conditional publication remains
unimplemented; authoring's host-publication instruction is not a claim of a
relay-enforced restriction on the agent.
